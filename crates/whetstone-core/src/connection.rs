//! Connections: the material Whetstone adds to a change's evidence beyond
//! the files it touches.
//!
//! A [`Connection`] is one piece of it, in one of three groups: connected
//! code (what the touched files call, are called by, link to or name), a
//! related test, or related configuration. Whetstone finds them at the
//! change's commit, and [`Evidence::build`] applies the exclusion rules and
//! the secret check to them as it does to touched files.
//!
//! [`Evidence::build`]: crate::Evidence::build

use std::collections::BTreeMap;
use std::fmt;

use crate::change::FileContent;

/// One piece of material Whetstone adds beyond the touched files: a range
/// of lines in a file at the change's commit, or the whole file, with why it
/// is there.
///
/// It keeps only what it would send, the lines of its range or the whole
/// file, and its `Debug` output shows neither that text nor a callable's
/// name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    id: ConnectionId,
    group: ConnectionGroup,
    tier: Tier,
    relations: Vec<Relation>,
    /// What it would send: the lines of its range, or the whole file. Only
    /// text has any.
    content: FileContent,
}

/// What finding a change's connections has given so far: the connections,
/// or why there are none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connections {
    /// They are still being found.
    Finding,
    /// These were found, none of them in a touched file, each with its own
    /// path and extent. Finding none is [`Connections::NoneFound`].
    Found(Vec<Connection>),
    /// None were found for this change.
    NoneFound,
    /// Finding them took longer than the time limit allows.
    TookTooLong,
    /// The analysis that finds them failed.
    AnalysisFailed,
}

/// Identifies a connection in the evidence and in the learner's choices: its
/// path and its extent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId {
    path: String,
    extent: Extent,
}

/// How much of its file a connection sends. It reads as the outbound review
/// shows it:
///
/// ```
/// use whetstone_core::{Extent, LineRange};
///
/// assert_eq!(Extent::Lines(LineRange::new(12, 30)?).to_string(), "lines 12–30");
/// assert_eq!(Extent::Lines(LineRange::new(7, 7)?).to_string(), "line 7");
/// assert_eq!(Extent::WholeFile.to_string(), "whole file");
/// # Ok::<(), whetstone_core::InvalidLineRange>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Extent {
    /// These lines of it.
    Lines(LineRange),
    /// All of it.
    WholeFile,
}

/// Lines of a file at the change's commit, numbered from 1 as in the file,
/// from the first to the last, both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineRange {
    first: usize,
    last: usize,
}

/// The lines given for a range are not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "lines {first} to {last} are not a line range: lines count from 1, and the first is not after the last"
)]
pub struct InvalidLineRange {
    first: usize,
    last: usize,
}

/// Which kind of material a connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionGroup {
    /// What the touched files call, are called by, link to or name.
    ConnectedCode,
    /// A test that conventionally belongs to a touched file.
    RelatedTest,
    /// The build manifest nearest a touched file.
    RelatedConfiguration,
}

/// How sure Whetstone is of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Resolved with high confidence, or linked to directly.
    High,
    /// Resolved with medium confidence, or named in passing.
    Medium,
}

/// Why a connection is there: what it connects to, and the touched file
/// that is in. It reads as the outbound review shows it:
///
/// ```
/// use whetstone_core::Relation;
///
/// let caller = Relation::Calls {
///     callable: "charge".to_owned(),
///     touched_file: "src/billing.rs".to_owned(),
/// };
///
/// assert_eq!(caller.to_string(), "calls `charge`");
/// assert_eq!(caller.touched_file(), "src/billing.rs");
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Relation {
    /// It calls `callable`, which the change adds or modifies in
    /// `touched_file`: it is a caller.
    Calls {
        /// The changed callable's name.
        callable: String,
        /// The touched file the changed callable is in.
        touched_file: String,
    },
    /// `callable`, which the change adds or modifies in `touched_file`,
    /// calls it: it is a callee.
    CalledBy {
        /// The changed callable's name.
        callable: String,
        /// The touched file the changed callable is in.
        touched_file: String,
    },
    /// A line the change adds or modifies in `touched_file` links to it.
    LinkedFrom {
        /// The touched file that links to it.
        touched_file: String,
    },
    /// A line the change adds or modifies in `touched_file` names its path.
    NamedIn {
        /// The touched file that names it.
        touched_file: String,
    },
    /// It is the test that conventionally belongs to `touched_file`.
    TestFor {
        /// The touched file it tests.
        touched_file: String,
    },
    /// It is the build manifest nearest `touched_file`.
    ManifestFor {
        /// The touched file it builds.
        touched_file: String,
    },
}

impl Connection {
    /// The connection to `extent` of the file at `path`, relative to the
    /// repository's root, in `group` and `tier`, there for its `relations`.
    /// `content` is the whole file at the change's commit; the connection
    /// keeps only the lines of `extent`, with their own line endings. Lines
    /// of a range past the file's end are not there to send.
    #[must_use]
    pub fn new(
        path: String,
        extent: Extent,
        group: ConnectionGroup,
        tier: Tier,
        relations: Vec<Relation>,
        content: FileContent,
    ) -> Self {
        let content = match (extent, content) {
            (Extent::Lines(range), FileContent::Text(text)) => FileContent::Text(
                text.split_inclusive('\n')
                    .skip(range.first - 1)
                    .take(range.last - range.first + 1)
                    .collect(),
            ),
            (_, content) => content,
        };
        Self {
            id: ConnectionId { path, extent },
            group,
            tier,
            relations,
            content,
        }
    }

    /// Its path and extent, which identify it.
    #[must_use]
    pub fn id(&self) -> &ConnectionId {
        &self.id
    }

    /// Which kind of material it is.
    #[must_use]
    pub fn group(&self) -> ConnectionGroup {
        self.group
    }

    /// How sure Whetstone is of it.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// Why it is there.
    #[must_use]
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// What it would send: the lines of its extent, if its file is text.
    pub(crate) fn content(&self) -> &FileContent {
        &self.content
    }

    /// The bytes it would send, its path and its text; a binary file's
    /// size; or nothing for a submodule or a symbolic link, which have no
    /// content.
    pub(crate) fn size(&self) -> Option<usize> {
        match &self.content {
            FileContent::Text(text) => Some(self.id.path.len() + text.len()),
            FileContent::Binary { size } => Some(*size),
            FileContent::Submodule | FileContent::Symlink => None,
        }
    }
}

/// `connections` with each file that one of them sends whole sent once, so
/// that no line is sent twice: every other connection in that file, a range
/// of it or the whole file again, merges into that one. The merged
/// connection is in the first of its connections' groups of related test,
/// related configuration and connected code, and in the higher of their
/// tiers. It is there for each of their relations, once, in that order of
/// groups. A file that none of them sends whole keeps its connections as
/// they are. They come back in path and extent order.
#[must_use]
pub fn merge_into_whole_files(connections: Vec<Connection>) -> Vec<Connection> {
    let mut by_path: BTreeMap<String, Vec<Connection>> = BTreeMap::new();
    for connection in connections {
        by_path
            .entry(connection.id.path.clone())
            .or_default()
            .push(connection);
    }
    let mut merged: Vec<Connection> = by_path
        .into_values()
        .flat_map(|mut in_file| {
            if in_file
                .iter()
                .all(|connection| connection.id.extent != Extent::WholeFile)
            {
                return in_file;
            }
            in_file
                .sort_by_key(|connection| (related_first(connection.group), connection.id.extent));
            let tier = if in_file
                .iter()
                .any(|connection| connection.tier == Tier::High)
            {
                Tier::High
            } else {
                Tier::Medium
            };
            let mut relations: Vec<Relation> = Vec::new();
            for relation in in_file.iter().flat_map(|connection| &connection.relations) {
                if !relations.contains(relation) {
                    relations.push(relation.clone());
                }
            }
            let group = in_file[0].group;
            let whole = in_file
                .into_iter()
                .find(|connection| connection.id.extent == Extent::WholeFile)
                .expect("a file in which a connection is sent whole has that connection");
            vec![Connection {
                group,
                tier,
                relations,
                ..whole
            }]
        })
        .collect();
    merged.sort_by(|a, b| a.id.cmp(&b.id));
    merged
}

/// Where `group` comes among the groups a merged connection takes the first
/// of: related tests, then related configuration, then connected code.
fn related_first(group: ConnectionGroup) -> u8 {
    match group {
        ConnectionGroup::RelatedTest => 0,
        ConnectionGroup::RelatedConfiguration => 1,
        ConnectionGroup::ConnectedCode => 2,
    }
}

impl ConnectionId {
    /// Where its file is, relative to the repository's root.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// How much of its file it sends.
    #[must_use]
    pub fn extent(&self) -> Extent {
        self.extent
    }
}

impl Extent {
    /// The number in its file of the first line it sends.
    pub(crate) fn first_line(self) -> usize {
        match self {
            Self::Lines(range) => range.first,
            Self::WholeFile => 1,
        }
    }
}

impl fmt::Display for Extent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lines(LineRange { first, last }) if first == last => write!(f, "line {first}"),
            Self::Lines(LineRange { first, last }) => write!(f, "lines {first}–{last}"),
            Self::WholeFile => f.write_str("whole file"),
        }
    }
}

impl LineRange {
    /// Lines `first` to `last` of a file, both included, numbered from 1.
    pub fn new(first: usize, last: usize) -> Result<Self, InvalidLineRange> {
        if first == 0 || first > last {
            return Err(InvalidLineRange { first, last });
        }
        Ok(Self { first, last })
    }

    /// The first line's number.
    #[must_use]
    pub fn first(self) -> usize {
        self.first
    }

    /// The last line's number.
    #[must_use]
    pub fn last(self) -> usize {
        self.last
    }
}

impl Relation {
    /// The touched file it connects to.
    #[must_use]
    pub fn touched_file(&self) -> &str {
        match self {
            Self::Calls { touched_file, .. }
            | Self::CalledBy { touched_file, .. }
            | Self::LinkedFrom { touched_file }
            | Self::NamedIn { touched_file }
            | Self::TestFor { touched_file }
            | Self::ManifestFor { touched_file } => touched_file,
        }
    }

    /// Its kind, as the outbound review words it.
    fn kind(&self) -> &'static str {
        match self {
            Self::Calls { .. } => "calls",
            Self::CalledBy { .. } => "called by",
            Self::LinkedFrom { .. } => "linked from",
            Self::NamedIn { .. } => "named in",
            Self::TestFor { .. } => "test for",
            Self::ManifestFor { .. } => "manifest for",
        }
    }
}

/// Its kind, then the changed callable or the touched file it connects to:
/// "calls `charge`", "called by `refund`", "linked from `docs/guide.md`".
impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let target = match self {
            Self::Calls { callable, .. } | Self::CalledBy { callable, .. } => callable,
            Self::LinkedFrom { touched_file }
            | Self::NamedIn { touched_file }
            | Self::TestFor { touched_file }
            | Self::ManifestFor { touched_file } => touched_file,
        };
        write!(f, "{} `{target}`", self.kind())
    }
}

/// Shows its kind and touched file, but never a callable's name, which
/// Whetstone's log never records.
impl fmt::Debug for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Relation")
            .field("kind", &self.kind())
            .field("touched_file", &self.touched_file())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relation_texts(relations: &[Relation]) -> Vec<String> {
        relations.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn each_relation_says_what_it_connects_to() {
        let file = || "src/store.rs".to_owned();
        let relations = [
            Relation::Calls {
                callable: "charge".to_owned(),
                touched_file: file(),
            },
            Relation::CalledBy {
                callable: "refund".to_owned(),
                touched_file: file(),
            },
            Relation::LinkedFrom {
                touched_file: "docs/guide.md".to_owned(),
            },
            Relation::NamedIn {
                touched_file: "deploy.yml".to_owned(),
            },
            Relation::TestFor {
                touched_file: file(),
            },
            Relation::ManifestFor {
                touched_file: file(),
            },
        ];

        assert_eq!(
            relation_texts(&relations),
            [
                "calls `charge`",
                "called by `refund`",
                "linked from `docs/guide.md`",
                "named in `deploy.yml`",
                "test for `src/store.rs`",
                "manifest for `src/store.rs`",
            ]
        );
    }

    /// `relations` in the file at `path`, to `extent` of it, in `group` and
    /// `tier`, which holds `TEXT`.
    fn piece(
        path: &str,
        extent: Extent,
        group: ConnectionGroup,
        tier: Tier,
        relations: Vec<Relation>,
    ) -> Connection {
        Connection::new(
            path.to_owned(),
            extent,
            group,
            tier,
            relations,
            FileContent::Text(TEXT.to_owned()),
        )
    }

    /// Every file's text here.
    const TEXT: &str = "one\ntwo\nthree\n";

    fn lines(first: usize, last: usize) -> Extent {
        Extent::Lines(LineRange::new(first, last).expect("the range is valid"))
    }

    fn calls(callable: &str) -> Relation {
        Relation::Calls {
            callable: callable.to_owned(),
            touched_file: "src/store.rs".to_owned(),
        }
    }

    /// Each connection as its path, extent, group, tier and relations.
    fn described(connections: &[Connection]) -> Vec<String> {
        connections
            .iter()
            .map(|connection| {
                format!(
                    "{} {} {:?} {:?}: {}",
                    connection.id().path(),
                    connection.id().extent(),
                    connection.group(),
                    connection.tier(),
                    relation_texts(connection.relations()).join(", ")
                )
            })
            .collect()
    }

    #[test]
    fn a_file_sent_whole_absorbs_the_ranges_in_it_and_a_related_group_comes_first() {
        let test_for = Relation::TestFor {
            touched_file: "src/store.rs".to_owned(),
        };
        let found = vec![
            piece(
                "tests/store.rs",
                lines(2, 3),
                ConnectionGroup::ConnectedCode,
                Tier::Medium,
                vec![calls("save"), calls("load")],
            ),
            piece(
                "src/api.rs",
                lines(1, 2),
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![calls("save")],
            ),
            piece(
                "tests/store.rs",
                Extent::WholeFile,
                ConnectionGroup::RelatedTest,
                Tier::High,
                vec![test_for],
            ),
            piece(
                "tests/store.rs",
                lines(1, 1),
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![calls("save")],
            ),
        ];

        let merged = merge_into_whole_files(found);

        assert_eq!(
            described(&merged),
            [
                "src/api.rs lines 1–2 ConnectedCode High: calls `save`",
                "tests/store.rs whole file RelatedTest High: \
                 test for `src/store.rs`, calls `save`, calls `load`",
            ]
        );
        assert_eq!(merged[1].content(), &FileContent::Text(TEXT.to_owned()));
    }

    #[test]
    fn two_connections_to_a_whole_file_are_one_in_the_higher_tier() {
        let named = Relation::NamedIn {
            touched_file: "deploy.yml".to_owned(),
        };
        let found = vec![
            piece(
                "config/limits.toml",
                Extent::WholeFile,
                ConnectionGroup::ConnectedCode,
                Tier::Medium,
                vec![named.clone()],
            ),
            piece(
                "config/limits.toml",
                Extent::WholeFile,
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![calls("limit"), named],
            ),
        ];

        assert_eq!(
            described(&merge_into_whole_files(found)),
            ["config/limits.toml whole file ConnectedCode High: \
              named in `deploy.yml`, calls `limit`"]
        );
    }

    #[test]
    fn a_line_range_counts_from_one_and_never_runs_backwards() {
        assert!(LineRange::new(0, 3).is_err());
        assert!(LineRange::new(5, 4).is_err());
        assert_eq!(LineRange::new(4, 4).map(LineRange::first), Ok(4));
    }

    #[test]
    fn a_connection_debugs_without_its_text_or_a_callable_name() {
        let connection = Connection::new(
            "src/billing.rs".to_owned(),
            Extent::WholeFile,
            ConnectionGroup::ConnectedCode,
            Tier::High,
            vec![Relation::Calls {
                callable: "charge_secretly".to_owned(),
                touched_file: "src/pay.rs".to_owned(),
            }],
            FileContent::Text("TOKEN=hunter2\n".to_owned()),
        );

        let debug = format!("{connection:?}");

        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(!debug.contains("charge_secretly"), "{debug}");
        assert!(debug.contains("src/billing.rs"), "{debug}");
        assert!(debug.contains("src/pay.rs"), "{debug}");
    }
}
