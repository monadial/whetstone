//! What Markdown links and path mentions read from a change: each touched
//! file's text at the commit and the lines the change adds or modifies, and
//! the repository files they find.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired into the outbound review by the #62 integration"
    )
)]

use std::collections::{BTreeMap, BTreeSet};

use whetstone_core::{FileContent, TouchedFile};

/// A touched file as links and mentions read it: its path, its text at the
/// commit if it has text there, and the lines the change adds or modifies.
#[derive(Debug, Clone)]
pub(crate) struct TouchedText<'a> {
    path: &'a str,
    /// None for a file that is binary, a symlink or a submodule at the
    /// commit, or that the change deletes.
    text: Option<&'a str>,
    changed: BTreeSet<usize>,
}

impl<'a> TouchedText<'a> {
    /// The touched `file`, whose 1-based lines `changed` at the commit the
    /// change adds or modifies, as [`added_lines`] finds them in its diff.
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

/// The 1-based line numbers, at the commit, of the lines a file's unified
/// `diff` adds. A line the change modifies is one it removes and adds again,
/// so these are the lines the change adds or modifies. A hunk that only
/// removes lines adds none.
pub(crate) fn added_lines(diff: &str) -> BTreeSet<usize> {
    let mut added = BTreeSet::new();
    // The number, at the commit, of the next line in the current hunk; none
    // before the first hunk, among the headers.
    let mut next = None;
    for line in diff.lines() {
        if line.starts_with("@@") {
            next = hunk_start(line);
            continue;
        }
        let Some(number) = next else { continue };
        match line.as_bytes().first() {
            Some(b'+') => {
                added.insert(number);
                next = Some(number + 1);
            }
            Some(b' ') => next = Some(number + 1),
            _ => {}
        }
    }
    added
}

/// The number at the commit of the first line of the hunk headed `header`,
/// such as `@@ -10,3 +11,4 @@`.
fn hunk_start(header: &str) -> Option<usize> {
    let after = header.split(' ').find_map(|part| part.strip_prefix('+'))?;
    after.split(',').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_lines_are_numbered_as_the_file_is_at_the_commit() {
        let diff = "diff --git a/notes.md b/notes.md\n\
                    index 1111111..2222222 100644\n\
                    --- a/notes.md\n\
                    +++ b/notes.md\n\
                    @@ -1,4 +1,5 @@\n\
                    \x20one\n\
                    -two\n\
                    +deux\n\
                    +zwei\n\
                    \x20three\n\
                    \x20four\n\
                    @@ -10,3 +11,3 @@ ## Later\n\
                    \x20ten\n\
                    -eleven\n\
                    +onze\n\
                    \x20twelve\n";

        assert_eq!(added_lines(diff), BTreeSet::from([2, 3, 12]));
    }

    #[test]
    fn a_created_file_adds_every_line_and_a_removal_adds_none() {
        let created = "diff --git a/a.md b/a.md\n\
                       new file mode 100644\n\
                       --- /dev/null\n\
                       +++ b/a.md\n\
                       @@ -0,0 +1,3 @@\n\
                       +one\n\
                       +two\n\
                       +three\n\
                       \\ No newline at end of file\n";
        let removal = "diff --git a/b.md b/b.md\n\
                       --- a/b.md\n\
                       +++ b/b.md\n\
                       @@ -2,2 +1,0 @@\n\
                       -two\n\
                       -three\n";
        let binary = "diff --git a/c.png b/c.png\n\
                      Binary files a/c.png and b/c.png differ\n";

        assert_eq!(added_lines(created), BTreeSet::from([1, 2, 3]));
        assert_eq!(added_lines(removal), BTreeSet::new());
        assert_eq!(added_lines(binary), BTreeSet::new());
    }
}
