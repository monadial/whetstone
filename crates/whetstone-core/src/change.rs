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
    before: Option<String>,
    after: Option<String>,
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
}

impl TouchedFile {
    /// The file at `path`, relative to the project's root, with its contents
    /// `before` the change (`None` if the change creates it) and `after` it
    /// (`None` if the change deletes it).
    #[must_use]
    pub fn new(path: String, before: Option<String>, after: Option<String>) -> Self {
        Self {
            path,
            before,
            after,
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
        self.before.as_deref()
    }

    /// The file's contents after the change, or `None` if the change deletes
    /// it.
    #[must_use]
    pub fn after(&self) -> Option<&str> {
        self.after.as_deref()
    }
}
