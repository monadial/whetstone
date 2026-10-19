//! Loading a commit as a change: its message, each file it touches before
//! and after, and its unified diff as `git diff` prints it. Everything is
//! read from Git's object store at that commit, never from the working
//! tree, so uncommitted edits and branch switches cannot change it.

use std::fmt::Write as _;

use gix::bstr::ByteSlice as _;
use gix::diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};
use gix::diff::blob::{Algorithm, InternedInput, UnifiedDiff, diff_with_slider_heuristics};
use gix::object::tree::EntryKind;
use gix::object::tree::diff::ChangeDetached;
use whetstone_core::{Change, CommitId, Comparison, FileContent, TouchedFile};

use super::LoadError;

/// Writing to a `String` never fails.
const INFALLIBLE: &str = "writing to a String never fails";

/// Git's rename detection, as `git diff -M` does it by default: renames
/// only, no copies, at 50% similarity or more, empty files included. gix
/// compares every source with every destination only while there are at
/// most `limit` pairs, and Git while there are at most `diff.renameLimit`
/// squared, 1000 × 1000, so the limit is squared here to find what Git
/// finds.
fn rewrites() -> gix::diff::Rewrites {
    gix::diff::Rewrites {
        limit: 1000 * 1000,
        track_empty: true,
        ..gix::diff::Rewrites::default()
    }
}

/// One side of a touched file: where it is, what kind of entry, its object
/// and what it holds.
#[derive(Debug)]
struct Side {
    path: String,
    kind: EntryKind,
    id: gix::ObjectId,
    content: FileContent,
}

/// The commit `id` in `git` as a change: compared with its parent, with
/// its first parent if it is a merge, or with an empty tree if it is the
/// first commit.
pub(super) fn load(git: &gix::Repository, id: CommitId) -> Result<Change, LoadError> {
    let object_id = gix::ObjectId::from_hex(id.full().as_bytes())?;
    let commit = git.find_commit(object_id)?;
    let tree = commit.tree()?;
    let mut parents = commit.parent_ids();
    let (comparison, parent_tree) = match parents.next() {
        None => (Comparison::EmptyTree, None),
        Some(first) => (
            if parents.next().is_some() {
                Comparison::FirstParent
            } else {
                Comparison::Parent
            },
            Some(first.object()?.into_commit().tree()?),
        ),
    };
    // Explicit options, so the learner's `diff.renames` changes nothing.
    let options = gix::diff::Options::default().with_rewrites(Some(rewrites()));
    let changes = git.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), options)?;
    let short_len = id.short().len();
    let mut files = Vec::new();
    for change in changes {
        let (before, after) = match change {
            ChangeDetached::Addition {
                location,
                entry_mode,
                id,
                ..
            } => (None, side(git, &location, entry_mode, id)?),
            ChangeDetached::Deletion {
                location,
                entry_mode,
                id,
                ..
            } => (side(git, &location, entry_mode, id)?, None),
            ChangeDetached::Modification {
                location,
                previous_entry_mode,
                previous_id,
                entry_mode,
                id,
            } => {
                let before = side(git, &location, previous_entry_mode, previous_id)?;
                let after = side(git, &location, entry_mode, id)?;
                // A file that becomes a symbolic link or a submodule, or the
                // other way round, is a deletion and an addition, as Git
                // shows it, so the text on either side is not hidden.
                if kind_changes(previous_entry_mode.kind(), entry_mode.kind()) {
                    files.extend(touched_file(before, None, short_len));
                    (None, after)
                } else {
                    (before, after)
                }
            }
            ChangeDetached::Rewrite {
                source_location,
                source_entry_mode,
                source_id,
                location,
                entry_mode,
                id,
                copy,
                ..
            } => (
                // Copies are not tracked, but a copy would leave its source
                // as it was.
                if copy {
                    None
                } else {
                    side(git, &source_location, source_entry_mode, source_id)?
                },
                side(git, &location, entry_mode, id)?,
            ),
        };
        files.extend(touched_file(before, after, short_len));
    }
    // In path order, as `git diff` lists files. The sort is stable, so a
    // deletion stays before an addition at the same path.
    files.sort_by(|(a, _), (b, _): &(TouchedFile, String)| a.path().cmp(b.path()));
    let (files, diffs): (Vec<TouchedFile>, Vec<String>) = files.into_iter().unzip();
    let message = commit
        .message_raw_sloppy()
        .to_str_lossy()
        .trim_end()
        .to_owned();
    tracing::info!(commit = id.full(), files = files.len(), "loaded a commit");
    Ok(Change::from_commit(
        id,
        comparison,
        message,
        diffs.concat(),
        files,
    ))
}

/// The side of a touched file at `location`, an entry of `mode` holding the
/// object `id`, or `None` for a directory, which is not a file.
fn side(
    git: &gix::Repository,
    location: &gix::bstr::BString,
    mode: gix::object::tree::EntryMode,
    id: gix::ObjectId,
) -> Result<Option<Side>, LoadError> {
    let kind = mode.kind();
    let content = match kind {
        EntryKind::Tree => return Ok(None),
        // A submodule's ID names a commit in another repository, so it is
        // never looked up here.
        EntryKind::Commit => FileContent::Submodule,
        EntryKind::Link => FileContent::Symlink,
        EntryKind::Blob | EntryKind::BlobExecutable => {
            FileContent::from_bytes(git.find_blob(id)?.take_data())
        }
    };
    Ok(Some(Side {
        path: location.to_str_lossy().into_owned(),
        kind,
        id,
        content,
    }))
}

/// The touched file with sides `before` and `after`, and its diff, with
/// object IDs shortened to `short_len` digits. `None` if neither side is a
/// file.
fn touched_file(
    before: Option<Side>,
    after: Option<Side>,
    short_len: usize,
) -> Option<(TouchedFile, String)> {
    let diff = file_diff(before.as_ref(), after.as_ref(), short_len);
    let file = match (before, after) {
        (None, None) => return None,
        (None, Some(after)) => TouchedFile::created(after.path, after.content),
        (Some(before), None) => TouchedFile::deleted(before.path, before.content),
        (Some(before), Some(after)) if before.path == after.path => {
            TouchedFile::modified(after.path, before.content, after.content)
        }
        (Some(before), Some(after)) => {
            TouchedFile::renamed(before.path, after.path, before.content, after.content)
        }
    };
    Some((file, diff))
}

/// Whether an entry of `before` becoming one of `after` changes what kind
/// of entry it is: a file, a symbolic link, a submodule or a directory. An
/// executable bit is only a mode.
fn kind_changes(before: EntryKind, after: EntryKind) -> bool {
    let class = |kind| match kind {
        EntryKind::Blob | EntryKind::BlobExecutable => 0,
        EntryKind::Link => 1,
        EntryKind::Commit => 2,
        EntryKind::Tree => 3,
    };
    class(before) != class(after)
}

/// The file mode Git writes for an entry of `kind`.
fn mode(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Tree => "040000",
        EntryKind::Blob => "100644",
        EntryKind::BlobExecutable => "100755",
        EntryKind::Link => "120000",
        EntryKind::Commit => "160000",
    }
}

/// The diff of one touched file, as `git diff` prints it: its `diff --git`
/// line and extended headers, then its hunks. Binary files say they differ,
/// and submodules and symlinks show no content. Unlike Git, a hunk header
/// names no function, a rename gives no similarity index, and paths are
/// never quoted.
fn file_diff(before: Option<&Side>, after: Option<&Side>, short_len: usize) -> String {
    let (Some(old), Some(new)) = (before.or(after), after.or(before)) else {
        return String::new();
    };
    let mut out = format!("diff --git a/{} b/{}\n", old.path, new.path);
    match (before, after) {
        (None, _) => writeln!(out, "new file mode {}", mode(new.kind)).expect(INFALLIBLE),
        (_, None) => writeln!(out, "deleted file mode {}", mode(old.kind)).expect(INFALLIBLE),
        (Some(_), Some(_)) if old.kind != new.kind => write!(
            out,
            "old mode {}\nnew mode {}\n",
            mode(old.kind),
            mode(new.kind)
        )
        .expect(INFALLIBLE),
        (Some(_), Some(_)) => {}
    }
    if old.path != new.path {
        write!(out, "rename from {}\nrename to {}\n", old.path, new.path).expect(INFALLIBLE);
    }
    let object = |side: Option<&Side>| {
        side.map_or_else(
            || "0".repeat(short_len),
            |side| side.id.to_hex_with_len(short_len).to_string(),
        )
    };
    if before.map(|side| side.id) == after.map(|side| side.id) {
        return out;
    }
    write!(out, "index {}..{}", object(before), object(after)).expect(INFALLIBLE);
    if before.is_some() && after.is_some() && old.kind == new.kind {
        write!(out, " {}", mode(new.kind)).expect(INFALLIBLE);
    }
    out.push('\n');
    let label = |prefix: &str, side: Option<&Side>| {
        side.map_or_else(
            || "/dev/null".to_owned(),
            |side| format!("{prefix}/{}", side.path),
        )
    };
    let binary = |side: Option<&Side>| {
        side.is_some_and(|side| matches!(side.content, FileContent::Binary { .. }))
    };
    match (text_of(before), text_of(after)) {
        (Some(before_text), Some(after_text)) => {
            let hunks = unified(before_text, after_text);
            if !hunks.is_empty() {
                write!(
                    out,
                    "--- {}\n+++ {}\n{hunks}",
                    label("a", before),
                    label("b", after)
                )
                .expect(INFALLIBLE);
            }
        }
        _ if binary(before) || binary(after) => writeln!(
            out,
            "Binary files {} and {} differ",
            label("a", before),
            label("b", after)
        )
        .expect(INFALLIBLE),
        _ => {}
    }
    out
}

/// The text of `side`, empty if there is no such side, or `None` if it is
/// not text.
fn text_of(side: Option<&Side>) -> Option<&str> {
    side.map_or(Some(""), |side| side.content.text())
}

/// The hunks of a unified diff from `before` to `after`, with three lines
/// of context, as `git diff` finds them: Myers with the indent heuristic.
fn unified(before: &str, after: &str) -> String {
    let input = InternedInput::new(before.as_bytes(), after.as_bytes());
    let diff = diff_with_slider_heuristics(Algorithm::Myers, &input);
    UnifiedDiff::new(
        &diff,
        &input,
        GitHunks::default(),
        ContextSize::symmetrical(3),
    )
    .consume()
    .expect(INFALLIBLE)
}

/// Writes hunks as Git does: a length of 1 left out of a hunk header, an
/// empty side as `-0,0`, and `\ No newline at end of file` after a last
/// line without one.
#[derive(Debug, Default)]
struct GitHunks(String);

impl ConsumeHunk for GitHunks {
    type Out = String;

    fn consume_hunk(
        &mut self,
        header: HunkHeader,
        lines: &[(DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        let range = |start: u32, len: u32| match len {
            0 => format!("{},0", start.saturating_sub(1)),
            1 => start.to_string(),
            len => format!("{start},{len}"),
        };
        let out = &mut self.0;
        writeln!(
            out,
            "@@ -{} +{} @@",
            range(header.before_hunk_start, header.before_hunk_len),
            range(header.after_hunk_start, header.after_hunk_len)
        )
        .expect(INFALLIBLE);
        for (kind, line) in lines {
            out.push(match kind {
                DiffLineKind::Context => ' ',
                DiffLineKind::Add => '+',
                DiffLineKind::Remove => '-',
            });
            out.push_str(&line.to_str_lossy());
            if !line.ends_with(b"\n") {
                out.push_str("\n\\ No newline at end of file\n");
            }
        }
        Ok(())
    }

    fn finish(self) -> String {
        self.0
    }
}
