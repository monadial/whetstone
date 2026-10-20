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
