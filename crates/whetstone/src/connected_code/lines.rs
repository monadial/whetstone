//! Where a change touches each file it leaves at its commit, read from the
//! change's unified diff: the lines it adds or modifies, and where it only
//! removes lines. Changed callables count both; links and mentions count
//! only the lines it adds or modifies.

use std::collections::{BTreeSet, HashMap};

use whetstone_core::Change;

/// Where a change touches one file it leaves at its commit.
#[derive(Debug, Default)]
pub(super) struct Touched<'c> {
    /// Every line the change adds or modifies, ascending.
    lines: Vec<u32>,
    /// Every run of removed lines with nothing added in its place.
    removals: Vec<Removal>,
    /// The file's text at the commit, or nothing if it is not text.
    text: &'c str,
}

/// A run of removed lines with nothing added in its place.
#[derive(Debug, Clone, Copy)]
struct Removal {
    /// The line at the commit the removal sits after, as a hunk header
    /// numbers a removal: 0 at the very top of the file.
    after: u32,
    /// How far its first line that is not blank was indented, if it has
    /// one.
    indent: Option<usize>,
}

/// For each file the change leaves at its commit (created, modified or
/// renamed, but not deleted), where it touches it. A file the change only
/// renames or changes the mode of is touched nowhere.
pub(super) fn touched(change: &Change) -> HashMap<&str, Touched<'_>> {
    change
        .files()
        .iter()
        .zip(change.file_diffs())
        .filter_map(|(file, diff)| {
            let text = file.after()?.text().unwrap_or_default();
            Some((file.path(), Touched::read(diff, text)))
        })
        .collect()
}

/// The 1-based line numbers, at the commit, of the lines a file's unified
/// `diff` adds or modifies. A line the change modifies is one it removes and
/// adds again. Unlike a changed callable, a hunk that only removes lines
/// touches no line here, so a link or a path on the line next to a removal
/// does not count.
pub(super) fn added_lines(diff: &str) -> BTreeSet<usize> {
    Touched::read(diff, "")
        .lines
        .into_iter()
        .map(|line| usize::try_from(line).expect("a line number fits in usize"))
        .collect()
}

impl<'c> Touched<'c> {
    /// Whether the change touches the lines from `start` to `end`,
    /// inclusive, which a callable at the commit covers: if it adds or
    /// modifies one of them, or removes lines inside them.
    ///
    /// A removal sits between two lines. It is inside the callable if the
    /// callable covers both. It is also inside if the callable covers the
    /// line before it and the removed code was indented deeper than the
    /// callable's first line, as when the last statements of a Python
    /// function are removed. So removing a whole function never touches
    /// the one before it.
    pub(super) fn touches(&self, start: u32, end: u32) -> bool {
        let first_from_start = self.lines.partition_point(|&line| line < start);
        if self
            .lines
            .get(first_from_start)
            .is_some_and(|&line| line <= end)
        {
            return true;
        }
        self.removals.iter().any(|removal| {
            (start..=end).contains(&removal.after)
                && (removal.after < end
                    || removal
                        .indent
                        .is_some_and(|indent| indent > self.indent_of(start)))
        })
    }

    /// Reads one file's diff, of a file holding `text` at the commit.
    fn read(diff: &str, text: &'c str) -> Self {
        let mut touched = Self {
            text,
            ..Self::default()
        };
        // The new side's next line number, once inside a hunk.
        let mut next: Option<u32> = None;
        // The run of `-` and `+` lines so far: whether it adds, and its
        // removal if it removes.
        let mut adds = false;
        let mut removal: Option<Removal> = None;
        for line in diff.lines() {
            if let Some(header) = line.strip_prefix("@@ ") {
                touched.end_run(&mut adds, &mut removal);
                next = new_start(header);
                continue;
            }
            let Some(number) = next.as_mut() else {
                continue;
            };
            match line.as_bytes().first() {
                Some(b'+') => {
                    touched.lines.push(*number);
                    *number += 1;
                    adds = true;
                }
                Some(b'-') => {
                    let removed = line.get(1..).unwrap_or_default();
                    let run = removal.get_or_insert(Removal {
                        after: number.saturating_sub(1),
                        indent: None,
                    });
                    if run.indent.is_none() && !removed.trim().is_empty() {
                        run.indent = Some(indent(removed));
                    }
                }
                // `\ No newline at end of file` belongs to the line before it.
                Some(b'\\') => {}
                // Context, which a tool that trims trailing spaces may leave
                // empty.
                _ => {
                    touched.end_run(&mut adds, &mut removal);
                    *number += 1;
                }
            }
        }
        touched.end_run(&mut adds, &mut removal);
        touched
    }

    /// Ends a run of `-` and `+` lines: a run that only removes is a
    /// removal.
    fn end_run(&mut self, adds: &mut bool, removal: &mut Option<Removal>) {
        if let Some(removal) = removal.take()
            && !*adds
        {
            self.removals.push(removal);
        }
        *adds = false;
    }

    /// How far the line `number` of the file at the commit is indented.
    fn indent_of(&self, number: u32) -> usize {
        let index = usize::try_from(number.saturating_sub(1)).unwrap_or(usize::MAX);
        self.text.lines().nth(index).map_or(0, indent)
    }
}

/// How many spaces and tabs `line` starts with.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// The new side's first line number in a hunk header after its `@@ `, such
/// as `-12,3 +14,5 @@`. An empty new side, `+13,0`, names the line before
/// it, so its next line is one more.
fn new_start(header: &str) -> Option<u32> {
    let new_side = header.split(' ').nth(1)?.strip_prefix('+')?;
    let (start, len) = new_side.split_once(',').unwrap_or((new_side, "1"));
    let start: u32 = start.parse().ok()?;
    Some(if len == "0" { start + 1 } else { start })
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
