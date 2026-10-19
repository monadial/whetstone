//! Changes: the commits a Practice is built from.

use crate::area::Area;

/// The commit a Practice is built from: its message, the areas it touches,
/// its diff and the code it touches, before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    message: String,
    areas: Vec<Area>,
    diff: String,
    files: Vec<TouchedFile>,
}

/// A file a change touches, as it was before the change and as the change
/// left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedFile {
    path: String,
    versions: Versions,
}

/// A touched file's contents on each side of the change.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Versions {
    Created { after: String },
    Modified { before: String, after: String },
    Deleted { before: String },
}

impl Change {
    /// A change with its commit `message`, the `areas` it touches, its
    /// unified `diff` and the `files` it touches.
    #[must_use]
    pub fn new(message: String, areas: Vec<Area>, diff: String, files: Vec<TouchedFile>) -> Self {
        Self {
            message,
            areas,
            diff,
            files,
        }
    }

    /// The whole commit message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The message's first line, which says what the change does.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }

    /// The areas the change touches.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
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
    /// creates with the contents `after`.
    #[must_use]
    pub fn created(path: String, after: String) -> Self {
        Self {
            path,
            versions: Versions::Created { after },
        }
    }

    /// The file at `path`, which the change edits from `before` to `after`.
    #[must_use]
    pub fn modified(path: String, before: String, after: String) -> Self {
        Self {
            path,
            versions: Versions::Modified { before, after },
        }
    }

    /// The file at `path`, which the change deletes from `before`.
    #[must_use]
    pub fn deleted(path: String, before: String) -> Self {
        Self {
            path,
            versions: Versions::Deleted { before },
        }
    }

    /// Where the file is, relative to the project's root.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The file's contents before the change, or `None` if the change
    /// creates it.
    #[must_use]
    pub fn before(&self) -> Option<&str> {
        match &self.versions {
            Versions::Created { .. } => None,
            Versions::Modified { before, .. } | Versions::Deleted { before } => Some(before),
        }
    }

    /// The file's contents after the change, or `None` if the change deletes
    /// it.
    #[must_use]
    pub fn after(&self) -> Option<&str> {
        match &self.versions {
            Versions::Created { after } | Versions::Modified { after, .. } => Some(after),
            Versions::Deleted { .. } => None,
        }
    }
}
