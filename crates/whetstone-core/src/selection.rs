//! Ranking and selection: which of a change's connections its evidence
//! keeps, and which it only counts because they did not fit.
//!
//! `rank` puts the most relevant first, and `select` walks the ranked
//! connections once. It keeps each one a rule leaves out, using no room,
//! and adds each one the rules send while it fits: in the room left under
//! [`Evidence::SIZE_LIMIT`] after the change, and if it is connected code,
//! within the connected code limit. One that does not fit is skipped whole,
//! never cut, and counted under a [`NotSent`] reason.
//!
//! **The selection is fixed by the first build.** Selection runs once, when
//! the connections arrive, and it reads only the change, the rules, the
//! selected areas and the limit, never the learner's [`ReviewChoices`]: the
//! room is what the rules alone leave after the change, and a connection
//! counts as sent when the rules alone send it. So every later build of the
//! same change's evidence, after any choice, selects the same connections,
//! with no selection to keep and pass back in. Leaving a connection or a
//! touched file out frees its room without adding anything else, and
//! bringing one back that a rule leaves out sends it beside those selected.
//!
//! [`Evidence::SIZE_LIMIT`]: crate::Evidence::SIZE_LIMIT
//! [`ReviewChoices`]: crate::ReviewChoices

use std::cmp::Reverse;
use std::fmt;

use crate::connected_code_limit::ConnectedCodeLimit;
use crate::connection::{Connection, ConnectionGroup, Extent, Tier};
use crate::evidence::FileStatus;

/// Why a connection no rule leaves out is not sent: it did not fit when
/// its change's connections arrived. It reads as the outbound review shows
/// it:
///
/// ```
/// use whetstone_core::NotSent;
///
/// assert_eq!(
///     NotSent::OverConnectedCodeLimit.to_string(),
///     "over the connected code limit"
/// );
/// assert_eq!(NotSent::NoRoom.to_string(), "no room under the size limit");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotSent {
    /// It is connected code, and it did not fit within the connected code
    /// limit beside the connected code before it.
    OverConnectedCodeLimit,
    /// It did not fit in the room left under
    /// [`Evidence::SIZE_LIMIT`](crate::Evidence::SIZE_LIMIT) after the change
    /// and the connections before it.
    NoRoom,
}

impl fmt::Display for NotSent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OverConnectedCodeLimit => "over the connected code limit",
            Self::NoRoom => "no room under the size limit",
        })
    }
}

/// What the selection keeps of a change's ranked connections.
#[derive(Debug)]
pub(crate) struct Selection<'a> {
    /// The connections the evidence lists, in rank order, with their
    /// status: each one that fit, and each one a rule leaves out.
    pub(crate) listed: Vec<(&'a Connection, FileStatus)>,
    /// How many did not fit, for each reason that has any, in the order
    /// the outbound review shows them.
    pub(crate) not_sent: Vec<(NotSent, usize)>,
}

/// `connections` from the most relevant to the least: related tests and
/// related configuration first, by path; then connected code, the high tier
/// before the medium, then the closest to a touched file it connects to
/// first; then by path and starting line.
pub(crate) fn rank(connections: &[Connection]) -> Vec<&Connection> {
    let mut ranked: Vec<&Connection> = connections.iter().collect();
    ranked.sort_by_cached_key(|&connection| Rank::of(connection));
    ranked
}

/// Walks `ranked`, connections with their status, once, keeping each one a
/// rule leaves out, which uses no room, and adding each one the rules send
/// while it fits: in `room`, and if it is connected code, within `limit`
/// beside the connected code before it. One that does not fit is skipped
/// whole and counted, and smaller ones after it can still be added.
///
/// It reads only whether the rules send each one, which the learner's
/// choices never change, so a choice never changes what is selected.
pub(crate) fn select(
    ranked: Vec<(&Connection, FileStatus)>,
    mut room: usize,
    limit: ConnectedCodeLimit,
) -> Selection<'_> {
    let mut connected_code_room = limit.bytes();
    let mut over_limit = 0;
    let mut no_room = 0;
    let mut listed = Vec::new();
    for (connection, status) in ranked {
        if status.sent_by_rules() {
            let size = connection
                .size()
                .expect("a connection the rules send is text, which has a size");
            let connected_code = connection.group() == ConnectionGroup::ConnectedCode;
            if size > room {
                no_room += 1;
                continue;
            }
            if connected_code && size > connected_code_room {
                over_limit += 1;
                continue;
            }
            room -= size;
            if connected_code {
                connected_code_room -= size;
            }
        }
        listed.push((connection, status));
    }
    Selection {
        listed,
        not_sent: [
            (NotSent::OverConnectedCodeLimit, over_limit),
            (NotSent::NoRoom, no_room),
        ]
        .into_iter()
        .filter(|&(_, count)| count > 0)
        .collect(),
    }
}

/// What a connection is ranked by, the smallest first, compared field by
/// field.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    Related,
    HighTier { closeness: Reverse<usize> },
    MediumTier { closeness: Reverse<usize> },
}

impl<'a> Rank<'a> {
    /// What `connection` is ranked by.
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
    use super::NotSent;
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

    /// The related test at `path` for [`PAY`], which sends `size` bytes.
    fn test_for_pay(path: &str, size: usize) -> Connection {
        connection(
            path,
            ConnectionGroup::RelatedTest,
            Tier::High,
            Relation::TestFor {
                touched_file: PAY.to_owned(),
            },
            sized(path, size),
        )
    }

    /// The paths of the connections `evidence` lists, in its order.
    fn paths(evidence: &Evidence) -> Vec<&str> {
        evidence
            .connections()
            .iter()
            .map(|connection| connection.id().path())
            .collect()
    }

    #[test]
    fn callers_beyond_the_connected_code_limit_are_not_sent_but_counted() {
        let mut found: Vec<Connection> = (0..10)
            .map(|n| caller(&format!("src/caller_{n}.rs"), 250))
            .collect();
        found.push(test_for_pay("tests/pay.rs", 2000));

        let evidence = build(found, limit(1));

        assert_eq!(
            paths(&evidence),
            [
                "tests/pay.rs",
                "src/caller_0.rs",
                "src/caller_1.rs",
                "src/caller_2.rs",
                "src/caller_3.rs",
            ]
        );
        assert_eq!(evidence.not_sent(), [(NotSent::OverConnectedCodeLimit, 6)]);
        // Related tests don't count towards the connected code limit.
        assert_eq!(evidence.connected_code_size(), 1000);
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
    fn connected_code_for_several_touched_files_ranks_by_the_closest() {
        let both = Connection::new(
            "web/cart/checkout.ts".to_owned(),
            Extent::WholeFile,
            ConnectionGroup::ConnectedCode,
            Tier::High,
            vec![
                Relation::Calls {
                    callable: "pay".to_owned(),
                    touched_file: PAY.to_owned(),
                },
                Relation::CalledBy {
                    callable: "total".to_owned(),
                    touched_file: "web/cart/total.ts".to_owned(),
                },
            ],
            FileContent::Text("x\n".to_owned()),
        );

        let evidence = build(vec![caller("src/api.rs", 100), both], limit(100));

        // `web/cart/` is closer to `web/cart/total.ts` than `src/` is to
        // the touched file `src/api.rs` connects to.
        assert_eq!(paths(&evidence), ["web/cart/checkout.ts", "src/api.rs"]);
    }

    /// The paths and statuses of the connections `evidence` lists, in its
    /// order.
    fn statuses(evidence: &Evidence) -> Vec<(&str, String)> {
        evidence
            .connections()
            .iter()
            .map(|connection| (connection.id().path(), connection.status().to_string()))
            .collect()
    }

    /// A change a little under 400 KB, and the room it leaves under the
    /// size limit.
    fn large_change() -> (Change, usize) {
        let change = change(&"x".repeat(198_000));
        let alone = build_with(&change, Vec::new(), limit(100), &ReviewChoices::default());
        (change, Evidence::SIZE_LIMIT - alone.size())
    }

    #[test]
    fn connections_fill_only_the_room_the_change_leaves_under_the_size_limit() {
        let (change, room) = large_change();
        let found = vec![
            test_for_pay("tests/pay.rs", room - 300),
            caller("src/api.rs", 301),
            caller("src/auth.rs", 300),
        ];

        let evidence = build_with(&change, found, limit(100), &ReviewChoices::default());

        assert_eq!(paths(&evidence), ["tests/pay.rs", "src/auth.rs"]);
        assert_eq!(evidence.not_sent(), [(NotSent::NoRoom, 1)]);
        assert_eq!(evidence.size(), Evidence::SIZE_LIMIT);
        assert_eq!(evidence.unsendable(), None);
    }

    #[test]
    fn a_change_over_the_size_limit_leaves_no_room_for_any_connection() {
        let change = change(&"x".repeat(200_000));

        let evidence = build_with(
            &change,
            vec![test_for_pay("tests/pay.rs", 100), caller("src/api.rs", 100)],
            limit(100),
            &ReviewChoices::default(),
        );

        assert_eq!(evidence.connections(), []);
        assert_eq!(evidence.not_sent(), [(NotSent::NoRoom, 2)]);
    }

    #[test]
    fn connections_over_the_limit_are_counted_before_those_with_no_room() {
        let (change, room) = large_change();
        let found = vec![caller("src/a.rs", room + 1), caller("src/b.rs", 1001)];

        let evidence = build_with(&change, found, limit(1), &ReviewChoices::default());

        assert_eq!(
            evidence.not_sent(),
            [(NotSent::OverConnectedCodeLimit, 1), (NotSent::NoRoom, 1)]
        );
    }

    /// Callers in rank order, of which the second is too large for a limit
    /// of 1 KB beside the first, and the third fits.
    fn callers() -> Vec<Connection> {
        vec![
            caller("src/a.rs", 600),
            caller("src/b.rs", 401),
            caller("src/c.rs", 400),
        ]
    }

    #[test]
    fn a_caller_too_large_for_the_limit_left_is_skipped_whole_while_a_smaller_later_one_fits() {
        let without = build(Vec::new(), limit(1));

        let evidence = build(callers(), limit(1));

        assert_eq!(paths(&evidence), ["src/a.rs", "src/c.rs"]);
        assert_eq!(evidence.not_sent(), [(NotSent::OverConnectedCodeLimit, 1)]);
        assert_eq!(evidence.connected_code_size(), 1000);
        assert_eq!(evidence.size(), without.size() + 1000);
    }

    #[test]
    fn leaving_a_connection_out_frees_its_room_without_pulling_another_in() {
        let change = change("fn pay() {}");
        let mut choices = ReviewChoices::default();
        let selected = build_with(&change, callers(), limit(1), &choices);

        choices.toggle_connection(&selected.connections()[0]);
        let evidence = build_with(&change, callers(), limit(1), &choices);

        assert_eq!(
            statuses(&evidence),
            [
                ("src/a.rs", "left out by you".to_owned()),
                ("src/c.rs", "included".to_owned()),
            ]
        );
        assert_eq!(evidence.not_sent(), [(NotSent::OverConnectedCodeLimit, 1)]);
        assert_eq!(evidence.connected_code_size(), 400);
    }

    #[test]
    fn leaving_the_touched_file_out_frees_its_room_without_pulling_a_connection_in() {
        let (change, room) = large_change();
        let found = || vec![caller("src/api.rs", room + 1)];
        let mut choices = ReviewChoices::default();
        let selected = build_with(&change, found(), limit(100), &choices);

        choices.toggle_file(&selected, selected.files()[0].index());
        let evidence = build_with(&change, found(), limit(100), &choices);

        assert_eq!(evidence.connections(), []);
        assert_eq!(evidence.not_sent(), [(NotSent::NoRoom, 1)]);
    }

    /// Related configuration: the manifest at `path` for [`PAY`], which
    /// sends `size` bytes.
    fn manifest(path: &str, size: usize) -> Connection {
        connection(
            path,
            ConnectionGroup::RelatedConfiguration,
            Tier::High,
            Relation::ManifestFor {
                touched_file: PAY.to_owned(),
            },
            sized(path, size),
        )
    }

    #[test]
    fn a_limit_of_0_sends_no_connected_code_but_still_related_tests_and_configuration() {
        let found = vec![
            caller("src/api.rs", 100),
            test_for_pay("tests/pay.rs", 100),
            manifest("Cargo.toml", 100),
        ];

        let evidence = build(found, limit(0));

        assert_eq!(paths(&evidence), ["Cargo.toml", "tests/pay.rs"]);
        assert_eq!(evidence.not_sent(), [(NotSent::OverConnectedCodeLimit, 1)]);
        assert_eq!(evidence.connected_code_size(), 0);
    }

    #[test]
    fn the_evidence_reports_a_limit_this_repository_lowered_and_selects_within_it() {
        let found = || vec![caller("src/a.rs", 30_000), caller("src/b.rs", 30_000)];
        let lowered = ConnectedCodeLimit::effective(Some(kb(250)), Some(kb(40)));

        let evidence = build(found(), lowered);

        let limit = evidence.connected_code_limit();
        assert_eq!(limit.kb(), kb(40));
        assert_eq!(
            limit.source(),
            ConnectedCodeLimitSource::LoweredByRepository
        );
        assert_eq!(paths(&evidence), ["src/a.rs"]);
        assert_eq!(evidence.not_sent(), [(NotSent::OverConnectedCodeLimit, 1)]);

        let learners = build(
            found(),
            ConnectedCodeLimit::effective(Some(kb(250)), Some(kb(400))),
        );
        assert_eq!(
            learners.connected_code_limit().source(),
            ConnectedCodeLimitSource::Learner
        );
        assert_eq!(learners.not_sent(), []);
    }

    /// Connected code in the high tier at `path` that [`PAY`] names,
    /// holding `content`.
    fn named(path: &str, content: FileContent) -> Connection {
        connection(
            path,
            ConnectionGroup::ConnectedCode,
            Tier::High,
            Relation::NamedIn {
                touched_file: PAY.to_owned(),
            },
            content,
        )
    }

    /// A secret path and a binary file, which rank before a caller and are
    /// left out by rules, and the caller.
    fn left_out_by_rules_then_caller() -> Vec<Connection> {
        vec![
            named("src/billing/.env", sized("src/billing/.env", 900)),
            named("src/billing/logo.png", FileContent::Binary { size: 5000 }),
            caller("src/api.rs", 900),
        ]
    }

    #[test]
    fn a_connection_a_rule_leaves_out_is_listed_with_its_reason_and_uses_no_room() {
        let evidence = build(left_out_by_rules_then_caller(), limit(1));

        assert_eq!(
            statuses(&evidence),
            [
                (
                    "src/billing/.env",
                    "left out: possible secret path".to_owned()
                ),
                ("src/billing/logo.png", "left out: binary file".to_owned()),
                ("src/api.rs", "included".to_owned()),
            ]
        );
        assert_eq!(evidence.not_sent(), []);
        assert_eq!(evidence.connected_code_size(), 900);
    }

    #[test]
    fn a_connection_brought_back_is_sent_beside_those_selected() {
        let change = change("fn pay() {}");
        let mut choices = ReviewChoices::default();
        let selected = build_with(&change, left_out_by_rules_then_caller(), limit(1), &choices);

        choices.toggle_connection(&selected.connections()[0]);
        let evidence = build_with(&change, left_out_by_rules_then_caller(), limit(1), &choices);

        assert_eq!(
            statuses(&evidence),
            [
                ("src/billing/.env", "included by you".to_owned()),
                ("src/billing/logo.png", "left out: binary file".to_owned()),
                ("src/api.rs", "included".to_owned()),
            ]
        );
        // Over the limit, by the learner's choice.
        assert_eq!(evidence.connected_code_size(), 1800);
    }
}
