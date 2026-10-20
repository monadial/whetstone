//! Changes: the commits a Practice is built from.

use std::borrow::Cow;
use std::fmt;

use crate::area::AreaId;

/// The commit a Practice is built from: its message, where it comes from,
/// its diff and the files it touches, before and after. A showcase change
/// names its areas; a commit from a repository has none, but carries its
/// commit ID and what it was compared with.
#[derive(Clone, PartialEq, Eq)]
pub struct Change {
    message: String,
    origin: Origin,
    diff: String,
    files: Vec<TouchedFile>,
}

/// Shows the message's and the diff's lengths, never their text, which may
/// hold a secret.
impl fmt::Debug for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Change")
            .field("message_bytes", &self.message.len())
            .field("origin", &self.origin)
            .field("diff_bytes", &self.diff.len())
            .field("files", &self.files)
            .finish()
    }
}

/// Where a change comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// One of the showcase project's curated changes, which names the areas
    /// it touches.
    Showcase {
        /// The IDs of the areas the change touches.
        areas: Vec<AreaId>,
    },
    /// A commit from the learner's repository.
    Commit {
        /// The commit's ID, which Whetstone shows but never sends.
        id: CommitId,
        /// What the commit was compared with to find its change.
        comparison: Comparison,
    },
}

/// What a commit was compared with to find its change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// Its only parent.
    Parent,
    /// The first of its parents: it is a merge, so its change is the whole
    /// of the merged work.
    FirstParent,
    /// An empty tree: it is a repository's first commit, so it creates every
    /// file it touches.
    EmptyTree,
}

/// A commit's identity: its full ID, 40 hexadecimal digits for SHA-1 or 64
/// for SHA-256, and how many of them its short ID shows, as `git log
/// --oneline` shortens it.
///
/// ```
/// use whetstone_core::CommitId;
///
/// let id = CommitId::new("419ace2d5c0e1f7b8a9d3e4f5a6b7c8d9e0f1a2b", 7)?;
///
/// assert_eq!(id.short(), "419ace2");
/// assert_eq!(id.full(), "419ace2d5c0e1f7b8a9d3e4f5a6b7c8d9e0f1a2b");
/// # Ok::<(), whetstone_core::InvalidCommitId>(())
/// ```
///
/// It is held inline, so it can be copied as freely as a number.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommitId {
    /// The full ID's hexadecimal digits, then zeros.
    hex: [u8; CommitId::LONGEST],
    /// How many digits the full ID has.
    len: u8,
    /// How many digits the short ID shows.
    short_len: u8,
}

/// The text given as a commit ID is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a commit ID: use 40 or 64 lowercase hexadecimal digits")]
pub struct InvalidCommitId(String);

impl CommitId {
    /// The digits in a SHA-256 commit ID, the longer kind.
    const LONGEST: usize = 64;
    /// The fewest digits a short ID shows, as in Git.
    const SHORTEST: usize = 4;

    /// The commit whose full ID is `full`, 40 or 64 lowercase hexadecimal
    /// digits, with a short ID of its first `short_len` digits. The short ID
    /// shows at least 4 digits and at most all of them.
    pub fn new(full: &str, short_len: usize) -> Result<Self, InvalidCommitId> {
        let digits = full.as_bytes();
        let valid = matches!(digits.len(), 40 | 64)
            && digits
                .iter()
                .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'));
        if !valid {
            return Err(InvalidCommitId(full.to_owned()));
        }
        let mut hex = [0; Self::LONGEST];
        hex[..digits.len()].copy_from_slice(digits);
        let len = u8::try_from(digits.len()).expect("a commit ID has at most 64 digits");
        let short_len = u8::try_from(short_len.clamp(Self::SHORTEST, digits.len()))
            .expect("a short ID has at most 64 digits");
        Ok(Self {
            hex,
            len,
            short_len,
        })
    }

    /// The full ID.
    #[must_use]
    pub fn full(&self) -> &str {
        self.digits(self.len)
    }

    /// The ID shortened as `git log --oneline` shortens it.
    #[must_use]
    pub fn short(&self) -> &str {
        self.digits(self.short_len)
    }

    /// The first `len` digits of the full ID.
    fn digits(&self, len: u8) -> &str {
        std::str::from_utf8(&self.hex[..usize::from(len)])
            .expect("a commit ID holds only hexadecimal digits")
    }
}

impl fmt::Debug for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CommitId").field(&self.full()).finish()
    }
}

/// A file a change touches, as it was before the change and as the change
/// left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedFile {
    /// Where the file is after the change, or where it was if the change
    /// deletes it.
    path: String,
    versions: Versions,
}

/// A touched file's contents on each side of the change.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Versions {
    Created {
        after: FileContent,
    },
    Modified {
        before: FileContent,
        after: FileContent,
    },
    Renamed {
        /// Where the file was before the change.
        from: String,
        before: FileContent,
        after: FileContent,
    },
    Deleted {
        before: FileContent,
    },
}

/// What one side of a touched file holds. Only text has content to show
/// and send; the other kinds are entries without it.
#[derive(Clone, PartialEq, Eq)]
pub enum FileContent {
    /// UTF-8 text.
    Text(String),
    /// Anything that is not text, by Git's heuristic: a NUL byte in the first
    /// 8,000 bytes, or bytes that are not valid UTF-8.
    Binary {
        /// The file's size in bytes.
        size: usize,
    },
    /// A submodule: another repository's commit, recorded here by its ID.
    Submodule,
    /// A symbolic link.
    Symlink,
}

/// Shows text's length, never the text.
impl fmt::Debug for FileContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.debug_struct("Text").field("bytes", &text.len()).finish(),
            Self::Binary { size } => f.debug_struct("Binary").field("size", size).finish(),
            Self::Submodule => f.write_str("Submodule"),
            Self::Symlink => f.write_str("Symlink"),
        }
    }
}

impl FileContent {
    /// How far into a file Git looks for a NUL byte to tell binary from
    /// text.
    const SNIFF: usize = 8000;

    /// A file holding `bytes`, as text or binary. It is binary if a NUL byte
    /// is among its first 8,000 bytes, as Git decides, or if it is not valid
    /// UTF-8, the only text encoding Whetstone reads:
    ///
    /// ```
    /// use whetstone_core::FileContent;
    ///
    /// assert_eq!(
    ///     FileContent::from_bytes(b"fn main() {}\n".to_vec()),
    ///     FileContent::Text("fn main() {}\n".to_owned())
    /// );
    /// assert_eq!(
    ///     FileContent::from_bytes(b"PNG\0\x01".to_vec()),
    ///     FileContent::Binary { size: 5 }
    /// );
    /// assert_eq!(
    ///     FileContent::from_bytes(b"caf\xe9\n".to_vec()),
    ///     FileContent::Binary { size: 5 }
    /// );
    /// ```
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let size = bytes.len();
        if bytes.iter().take(Self::SNIFF).any(|&byte| byte == 0) {
            return Self::Binary { size };
        }
        String::from_utf8(bytes).map_or(Self::Binary { size }, Self::Text)
    }

    /// The text, if this is text.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Binary { .. } | Self::Submodule | Self::Symlink => None,
        }
    }
}

impl From<String> for FileContent {
    /// The text `text`.
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl Change {
    /// One of the showcase project's curated changes, with its commit
    /// `message`, the IDs of the `areas` it touches, its unified `diff` and
    /// the `files` it touches.
    #[must_use]
    pub fn new(message: String, areas: Vec<AreaId>, diff: String, files: Vec<TouchedFile>) -> Self {
        Self {
            message,
            origin: Origin::Showcase { areas },
            diff,
            files,
        }
    }

    /// The commit `id` from the learner's repository, compared as
    /// `comparison` says, with its `message`, its unified `diff` and the
    /// `files` it touches.
    #[must_use]
    pub fn from_commit(
        id: CommitId,
        comparison: Comparison,
        message: String,
        diff: String,
        files: Vec<TouchedFile>,
    ) -> Self {
        Self {
            message,
            origin: Origin::Commit { id, comparison },
            diff,
            files,
        }
    }

    /// The whole commit message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The message's subject, which says what the change does: its first
    /// paragraph, its lines joined by spaces, as `git log --format=%s`
    /// shows it.
    ///
    /// ```
    /// use whetstone_core::Change;
    ///
    /// let change = Change::new(
    ///     "Retry failed payments\nonce\n\nA declined card is tried again.".to_owned(),
    ///     Vec::new(),
    ///     String::new(),
    ///     Vec::new(),
    /// );
    ///
    /// assert_eq!(change.summary(), "Retry failed payments once");
    /// ```
    #[must_use]
    pub fn summary(&self) -> Cow<'_, str> {
        let mut paragraph = self
            .message
            .lines()
            .map(str::trim)
            .skip_while(|line| line.is_empty())
            .take_while(|line| !line.is_empty());
        let first = paragraph.next().unwrap_or_default();
        match paragraph.next() {
            None => Cow::Borrowed(first),
            Some(second) => Cow::Owned(
                [first, second]
                    .into_iter()
                    .chain(paragraph)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        }
    }

    /// Where the change comes from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The IDs of the areas the change touches. A commit from a repository
    /// names none.
    #[must_use]
    pub fn areas(&self) -> &[AreaId] {
        match &self.origin {
            Origin::Showcase { areas } => areas,
            Origin::Commit { .. } => &[],
        }
    }

    /// The change as a unified diff, every touched file in turn.
    #[must_use]
    pub fn diff(&self) -> &str {
        &self.diff
    }

    /// The files the change touches, in the diff's order.
    #[must_use]
    pub fn files(&self) -> &[TouchedFile] {
        &self.files
    }

    /// The diff of each file the change touches, from its `diff --git` line
    /// to the next, in the order of [`Change::files`]:
    ///
    /// ```
    /// use whetstone_core::Change;
    ///
    /// let change = Change::new(
    ///     "Tidy up".to_owned(),
    ///     Vec::new(),
    ///     "diff --git a/a b/a\n-x\n+y\ndiff --git a/b b/b\n+z\n".to_owned(),
    ///     Vec::new(),
    /// );
    ///
    /// assert_eq!(
    ///     change.file_diffs().collect::<Vec<_>>(),
    ///     ["diff --git a/a b/a\n-x\n+y\n", "diff --git a/b b/b\n+z\n"]
    /// );
    /// ```
    pub fn file_diffs(&self) -> impl Iterator<Item = &str> {
        const HEADER: &str = "\ndiff --git ";
        let mut rest = self.diff.as_str();
        std::iter::from_fn(move || {
            if rest.is_empty() {
                return None;
            }
            // Each file's diff ends with the newline before the next header.
            let end = rest
                .match_indices(HEADER)
                .next()
                .map_or(rest.len(), |(at, _)| at + 1);
            let (file_diff, after) = rest.split_at(end);
            rest = after;
            Some(file_diff)
        })
    }
}

impl TouchedFile {
    /// The file at `path`, relative to the project's root, which the change
    /// creates holding `after`.
    #[must_use]
    pub fn created(path: String, after: impl Into<FileContent>) -> Self {
        Self {
            path,
            versions: Versions::Created {
                after: after.into(),
            },
        }
    }

    /// The file at `path`, which the change edits from `before` to `after`.
    #[must_use]
    pub fn modified(
        path: String,
        before: impl Into<FileContent>,
        after: impl Into<FileContent>,
    ) -> Self {
        Self {
            path,
            versions: Versions::Modified {
                before: before.into(),
                after: after.into(),
            },
        }
    }

    /// The file the change moves `from` one path to `path`, and may edit on
    /// the way, from `before` to `after`.
    #[must_use]
    pub fn renamed(
        from: String,
        path: String,
        before: impl Into<FileContent>,
        after: impl Into<FileContent>,
    ) -> Self {
        Self {
            path,
            versions: Versions::Renamed {
                from,
                before: before.into(),
                after: after.into(),
            },
        }
    }

    /// The file at `path`, which the change deletes from `before`.
    #[must_use]
    pub fn deleted(path: String, before: impl Into<FileContent>) -> Self {
        Self {
            path,
            versions: Versions::Deleted {
                before: before.into(),
            },
        }
    }

    /// Where the file is, relative to the project's root: after the change,
    /// or before it if the change deletes it.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Where the file was before the change renamed it, if it did.
    #[must_use]
    pub fn renamed_from(&self) -> Option<&str> {
        match &self.versions {
            Versions::Renamed { from, .. } => Some(from),
            Versions::Created { .. } | Versions::Modified { .. } | Versions::Deleted { .. } => None,
        }
    }

    /// What the file held before the change, or `None` if the change
    /// creates it.
    #[must_use]
    pub fn before(&self) -> Option<&FileContent> {
        match &self.versions {
            Versions::Created { .. } => None,
            Versions::Modified { before, .. }
            | Versions::Renamed { before, .. }
            | Versions::Deleted { before } => Some(before),
        }
    }

    /// What the file holds after the change, or `None` if the change deletes
    /// it.
    #[must_use]
    pub fn after(&self) -> Option<&FileContent> {
        match &self.versions {
            Versions::Created { after }
            | Versions::Modified { after, .. }
            | Versions::Renamed { after, .. } => Some(after),
            Versions::Deleted { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_debugs_its_lengths_never_its_message_diff_or_file_text() {
        let change = Change::new(
            "Configure the token".to_owned(),
            Vec::new(),
            "+TOKEN=hunter2\n".to_owned(),
            vec![TouchedFile::created(
                ".env".to_owned(),
                FileContent::Text("TOKEN=hunter2\n".to_owned()),
            )],
        );

        let debug = format!("{change:?}");

        assert!(!debug.contains("Configure"), "{debug}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("message_bytes: 19"), "{debug}");
        assert!(debug.contains(".env"), "{debug}");
    }
}
