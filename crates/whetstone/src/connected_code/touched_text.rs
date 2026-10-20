//! What Markdown links and path mentions read from a change: each touched
//! file's text at the commit and the lines the change adds or modifies, and
//! the repository files they find.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use whetstone_core::{FileContent, TouchedFile};

/// A touched file as links and mentions read it: its path, its text at the
/// commit if it has text there, and the lines the change adds or modifies.
#[derive(Clone)]
pub(crate) struct TouchedText<'a> {
    path: &'a str,
    /// None for a file that is binary, a symlink or a submodule at the
    /// commit, or that the change deletes.
    text: Option<&'a str>,
    changed: BTreeSet<usize>,
}

/// Shows the text's length, never the text, which may hold a secret.
impl fmt::Debug for TouchedText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TouchedText")
            .field("path", &self.path)
            .field("text_bytes", &self.text.map(str::len))
            .field("changed", &self.changed)
            .finish()
    }
}

impl<'a> TouchedText<'a> {
    /// The touched `file`, whose 1-based lines `changed` at the commit the
    /// change adds or modifies, as [`added_lines`](super::lines::added_lines)
    /// finds them in its diff.
    pub(crate) fn new(file: &'a TouchedFile, changed: BTreeSet<usize>) -> Self {
        Self {
            path: file.path(),
            text: file.after().and_then(FileContent::text),
            changed,
        }
    }

    /// Where the file is, relative to the repository's root.
    pub(crate) fn path(&self) -> &'a str {
        self.path
    }

    /// The file's text at the commit, if it has text there.
    pub(crate) fn text(&self) -> Option<&'a str> {
        self.text
    }

    /// Whether the change adds or modifies the 1-based line `number`.
    pub(crate) fn is_changed(&self, number: usize) -> bool {
        self.changed.contains(&number)
    }

    /// The lines of the text that the change adds or modifies, in order.
    pub(crate) fn changed_lines(&self) -> impl Iterator<Item = &'a str> {
        self.text
            .into_iter()
            .flat_map(str::lines)
            .zip(1..)
            .filter(|(_, number)| self.changed.contains(number))
            .map(|(line, _)| line)
    }
}

/// A repository file that the change links to or names, which is sent
/// whole, with every touched file that links to it or names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The file's path, relative to the repository's root.
    pub(crate) path: String,
    /// The touched files that link to it or name it, in path order, each
    /// once.
    pub(crate) found_in: Vec<String>,
}

/// The targets in `found`, pairs of a target's path and a touched file it
/// was found in: one per path, in path order.
pub(crate) fn targets<'a>(found: impl IntoIterator<Item = (String, &'a str)>) -> Vec<Target> {
    let mut by_path = BTreeMap::<String, BTreeSet<&str>>::new();
    for (path, touched) in found {
        by_path.entry(path).or_default().insert(touched);
    }
    by_path
        .into_iter()
        .map(|(path, found_in)| Target {
            path,
            found_in: found_in.into_iter().map(str::to_owned).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_touched_text_debugs_its_length_never_its_text() {
        let file = TouchedFile::created(".env".to_owned(), "TOKEN=hunter2\n".to_owned());

        let debug = format!("{:?}", TouchedText::new(&file, BTreeSet::from([1])));

        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains(".env"), "{debug}");
        assert!(debug.contains("14"), "{debug}");
    }
}
