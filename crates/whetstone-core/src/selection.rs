//! Ranking and selection: which of a change's connections its evidence
//! keeps, and which it only counts because they did not fit.

use std::cmp::Reverse;

use crate::connection::{Connection, ConnectionGroup, Extent, Tier};

/// `connections` from the most relevant to the least: related tests and
/// related configuration first, by path; then connected code, the high tier
/// before the medium, then the closest to a touched file it connects to
/// first; then by path and starting line.
pub(crate) fn rank(connections: &[Connection]) -> Vec<&Connection> {
    let mut ranked: Vec<&Connection> = connections.iter().collect();
    ranked.sort_by_cached_key(|&connection| Rank::of(connection));
    ranked
}

/// What a connection is ranked by, the smallest first, compared field by
/// field.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Rank<'a> {
    standing: Standing,
    path: &'a str,
    first_line: usize,
    /// Breaks a tie between extents that start on the same line.
    extent: Extent,
}

/// How a connection ranks before its path and starting line: related tests
/// and related configuration first, then connected code in the high tier,
/// then in the medium, the closest first within each.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    Related,
    HighTier { closeness: Reverse<usize> },
    MediumTier { closeness: Reverse<usize> },
}

impl<'a> Rank<'a> {
    fn of(connection: &'a Connection) -> Self {
        let closeness = || Reverse(closeness(connection));
        let standing = match (connection.group(), connection.tier()) {
            (ConnectionGroup::RelatedTest | ConnectionGroup::RelatedConfiguration, _) => {
                Standing::Related
            }
            (ConnectionGroup::ConnectedCode, Tier::High) => Standing::HighTier {
                closeness: closeness(),
            },
            (ConnectionGroup::ConnectedCode, Tier::Medium) => Standing::MediumTier {
                closeness: closeness(),
            },
        };
        let id = connection.id();
        Self {
            standing,
            path: id.path(),
            first_line: id.extent().first_line(),
            extent: id.extent(),
        }
    }
}

/// How close `connection` is to the touched files: the most leading path
/// components its path shares with one it connects to.
fn closeness(connection: &Connection) -> usize {
    let path = connection.id().path();
    connection
        .relations()
        .iter()
        .map(|relation| {
            path.split('/')
                .zip(relation.touched_file().split('/'))
                .take_while(|(ours, theirs)| ours == theirs)
                .count()
        })
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::{
        Change, CommitId, Comparison, ConnectedCodeLimit, ConnectedCodeLimitKb,
        ConnectedCodeLimitSource, Connection, ConnectionGroup, Connections, Evidence,
        ExcludePatterns, Extent, FileContent, LineRange, Relation, ReviewChoices, Tier,
        TouchedFile,
    };

    /// The touched file every connection here connects to.
    const PAY: &str = "src/billing/pay.rs";

    /// A commit that creates [`PAY`], holding `content`.
    fn change(content: &str) -> Change {
        Change::from_commit(
            CommitId::new("419ace2d5c0e1f7b8a9d3e4f5a6b7c8d9e0f1a2b", 7).expect("the ID is valid"),
            Comparison::Parent,
            "Retry failed payments".to_owned(),
            format!("diff --git a/{PAY} b/{PAY}\n@@ -0,0 +1 @@\n+{content}\n"),
            vec![TouchedFile::created(
                PAY.to_owned(),
                FileContent::Text(format!("{content}\n")),
            )],
        )
    }

    fn kb(kb: i64) -> ConnectedCodeLimitKb {
        ConnectedCodeLimitKb::try_from(kb).expect("the limit is from 0 to 400")
    }

    /// The limit the learner sets to `learner` KB, in a repository that
    /// sets none.
    fn limit(learner: i64) -> ConnectedCodeLimit {
        ConnectedCodeLimit::effective(Some(kb(learner)), None)
    }

    fn build(connections: Vec<Connection>, limit: ConnectedCodeLimit) -> Evidence {
        build_with(
            &change("fn pay() {}"),
            connections,
            limit,
            &ReviewChoices::default(),
        )
    }

    fn build_with(
        change: &Change,
        connections: Vec<Connection>,
        limit: ConnectedCodeLimit,
        choices: &ReviewChoices,
    ) -> Evidence {
        Evidence::build(
            change,
            &ExcludePatterns::default(),
            &[],
            &Connections::Found(connections),
            limit,
            choices,
        )
    }

    /// The whole of `path` in `group` and `tier`, there for `relation`,
    /// holding `content`.
    fn connection(
        path: &str,
        group: ConnectionGroup,
        tier: Tier,
        relation: Relation,
        content: FileContent,
    ) -> Connection {
        Connection::new(
            path.to_owned(),
            Extent::WholeFile,
            group,
            tier,
            vec![relation],
            content,
        )
    }

    /// Text that makes a connection at `path` send `size` bytes in all,
    /// its path's and its text's.
    fn sized(path: &str, size: usize) -> FileContent {
        FileContent::Text("x".repeat(size - path.len()))
    }

    /// Connected code in the high tier that calls `pay` in [`PAY`] and
    /// sends `size` bytes.
    fn caller(path: &str, size: usize) -> Connection {
        connection(
            path,
            ConnectionGroup::ConnectedCode,
            Tier::High,
            Relation::Calls {
                callable: "pay".to_owned(),
                touched_file: PAY.to_owned(),
            },
            sized(path, size),
        )
    }

    /// The paths and extents of the connections `evidence` lists, in its
    /// order.
    fn listed(evidence: &Evidence) -> Vec<(&str, String)> {
        evidence
            .connections()
            .iter()
            .map(|connection| (connection.id().path(), connection.id().extent().to_string()))
            .collect()
    }

    fn lines(first: usize, last: usize) -> Extent {
        Extent::Lines(LineRange::new(first, last).expect("the range is valid"))
    }

    #[test]
    fn related_tests_and_configuration_come_first_then_connected_code_by_tier_closeness_and_place()
    {
        let test_for_pay = || Relation::TestFor {
            touched_file: PAY.to_owned(),
        };
        let code = |path: &str, tier| {
            connection(
                path,
                ConnectionGroup::ConnectedCode,
                tier,
                Relation::CalledBy {
                    callable: "pay".to_owned(),
                    touched_file: PAY.to_owned(),
                },
                FileContent::Text("x\n".to_owned()),
            )
        };
        let api = |extent| {
            Connection::new(
                "src/api.rs".to_owned(),
                extent,
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![Relation::Calls {
                    callable: "pay".to_owned(),
                    touched_file: PAY.to_owned(),
                }],
                FileContent::Text("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n".to_owned()),
            )
        };
        let found = vec![
            code("docs/pay.md", Tier::Medium),
            code("src/billing/ledger.rs", Tier::Medium),
            code("api/checkout.rs", Tier::High),
            code("src/auth.rs", Tier::High),
            api(lines(10, 12)),
            api(lines(3, 5)),
            code("src/billing/refund.rs", Tier::High),
            connection(
                "tests/pay.rs",
                ConnectionGroup::RelatedTest,
                Tier::Medium,
                test_for_pay(),
                FileContent::Text("x\n".to_owned()),
            ),
            connection(
                "src/billing/pay_test.rs",
                ConnectionGroup::RelatedTest,
                Tier::High,
                test_for_pay(),
                FileContent::Text("x\n".to_owned()),
            ),
            connection(
                "Cargo.toml",
                ConnectionGroup::RelatedConfiguration,
                Tier::High,
                Relation::ManifestFor {
                    touched_file: PAY.to_owned(),
                },
                FileContent::Text("x\n".to_owned()),
            ),
        ];

        let evidence = build(found, limit(100));

        assert_eq!(
            listed(&evidence),
            [
                // Related tests and configuration, by path alone.
                ("Cargo.toml", "whole file".to_owned()),
                ("src/billing/pay_test.rs", "whole file".to_owned()),
                ("tests/pay.rs", "whole file".to_owned()),
                // The high tier, sharing `src/billing/` with the touched file.
                ("src/billing/refund.rs", "whole file".to_owned()),
                // Sharing `src/`, by path, then by starting line.
                ("src/api.rs", "lines 3–5".to_owned()),
                ("src/api.rs", "lines 10–12".to_owned()),
                ("src/auth.rs", "whole file".to_owned()),
                // Sharing nothing.
                ("api/checkout.rs", "whole file".to_owned()),
                // The medium tier, by closeness too.
                ("src/billing/ledger.rs", "whole file".to_owned()),
                ("docs/pay.md", "whole file".to_owned()),
            ]
        );
    }

    #[test]
    fn the_evidence_reports_a_limit_this_repository_lowered() {
        let lowered = ConnectedCodeLimit::effective(Some(kb(250)), Some(kb(40)));

        let evidence = build(vec![caller("src/api.rs", 300)], lowered);

        let limit = evidence.connected_code_limit();
        assert_eq!(limit.kb(), kb(40));
        assert_eq!(
            limit.source(),
            ConnectedCodeLimitSource::LoweredByRepository
        );
    }
}
