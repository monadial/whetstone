//! A commit's whole tree: every file in it, with its kind, and the contents
//! of any blob by path. Everything is read from Git's object store at that
//! commit, never from the working tree, and nothing is written.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "connected code reads commit trees once the #58 integration wires it in"
    )
)]

use std::fmt;

use gix::bstr::ByteSlice as _;
use gix::object::tree::EntryKind;
use whetstone_core::CommitId;

use super::LoadError;

/// What a file in a commit's tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeEntryKind {
    /// A regular file.
    Blob,
    /// A regular file with its executable bit set.
    Executable,
    /// A symbolic link.
    Symlink,
    /// A submodule: another repository's commit, recorded here by its ID.
    Submodule,
}

/// One file in a commit's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeEntry {
    /// Where the file is, relative to the repository's root.
    path: String,
    kind: TreeEntryKind,
    /// The blob, symbolic link target or submodule commit it records.
    id: gix::ObjectId,
}

impl TreeEntry {
    /// Where the file is, relative to the repository's root, with `/`
    /// between directories.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// What the file is.
    pub(crate) fn kind(&self) -> TreeEntryKind {
        self.kind
    }
}

/// A commit's whole tree, listed once when it is opened. Blobs are read
/// only when asked for.
pub(crate) struct CommitTree<'r> {
    git: &'r gix::Repository,
    /// Every file, in path order. Directories are not files.
    entries: Vec<TreeEntry>,
}

/// Shows how many files there are, not what they are called.
impl fmt::Debug for CommitTree<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommitTree")
            .field("files", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// The tree of the commit `id` in `git`, with every file in it listed.
pub(super) fn open(git: &gix::Repository, id: CommitId) -> Result<CommitTree<'_>, LoadError> {
    let object_id = gix::ObjectId::from_hex(id.full().as_bytes())?;
    let tree = git.find_commit(object_id)?.tree()?;
    let mut entries: Vec<TreeEntry> = tree
        .traverse()
        .breadthfirst
        .files()?
        .into_iter()
        .filter_map(|entry| {
            let kind = match entry.mode.kind() {
                EntryKind::Tree => return None,
                EntryKind::Blob => TreeEntryKind::Blob,
                EntryKind::BlobExecutable => TreeEntryKind::Executable,
                EntryKind::Link => TreeEntryKind::Symlink,
                EntryKind::Commit => TreeEntryKind::Submodule,
            };
            Some(TreeEntry {
                path: entry.filepath.to_str_lossy().into_owned(),
                kind,
                id: entry.oid,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    // Two paths that are not UTF-8 can read the same once their invalid
    // bytes are replaced; the first is kept, so a path names one file.
    entries.dedup_by(|later, earlier| later.path == earlier.path);
    Ok(CommitTree { git, entries })
}

impl CommitTree<'_> {
    /// Every file in the tree, in path order. Directories are not listed.
    pub(crate) fn entries(&self) -> &[TreeEntry] {
        &self.entries
    }

    /// The contents of the file at `path` if it is a regular file, or
    /// `None` if there is no file there or it is a symbolic link or a
    /// submodule.
    pub(crate) fn read(&self, path: &str) -> Result<Option<Vec<u8>>, LoadError> {
        let Ok(at) = self
            .entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
        else {
            return Ok(None);
        };
        let entry = &self.entries[at];
        match entry.kind {
            TreeEntryKind::Blob | TreeEntryKind::Executable => {
                Ok(Some(self.git.find_blob(entry.id)?.take_data()))
            }
            TreeEntryKind::Symlink | TreeEntryKind::Submodule => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::test_repo::{TestRepo, files_under};

    #[test]
    fn a_commits_tree_lists_every_file_with_its_kind_and_reads_regular_files() {
        let repo = TestRepo::new();
        repo.write("README.md", "# Shapes\n")
            .write("src/shape.rs", "pub fn area() {}\n")
            .write("scripts/release", "#!/bin/sh\necho released\n")
            .executable("scripts/release")
            .symlink("latest", "src/shape.rs")
            .submodule(
                "vendor/geometry",
                "0123456789abcdef0123456789abcdef01234567",
            );
        let first = repo.commit("Start");
        repo.write("src/shape.rs", "pub fn area() -> u32 { 0 }\n")
            .write("src/edge.rs", "pub fn length() {}\n");
        repo.commit("Grow");
        let repository = repo.open();
        let before = files_under(&repo.root());

        let tree = repository
            .tree(first)
            .expect("read the first commit's tree");

        let listed: Vec<(&str, TreeEntryKind)> = tree
            .entries()
            .iter()
            .map(|entry| (entry.path(), entry.kind()))
            .collect();
        assert_eq!(
            listed,
            [
                ("README.md", TreeEntryKind::Blob),
                ("latest", TreeEntryKind::Symlink),
                ("scripts/release", TreeEntryKind::Executable),
                ("src/shape.rs", TreeEntryKind::Blob),
                ("vendor/geometry", TreeEntryKind::Submodule),
            ]
        );
        let read = |path| tree.read(path).expect("read a file");
        assert_eq!(read("src/shape.rs"), Some(b"pub fn area() {}\n".to_vec()));
        assert_eq!(
            read("scripts/release"),
            Some(b"#!/bin/sh\necho released\n".to_vec())
        );
        for not_a_regular_file in ["latest", "vendor/geometry", "src", "src/edge.rs"] {
            assert_eq!(read(not_a_regular_file), None, "{not_a_regular_file}");
        }
        assert_eq!(files_under(&repo.root()), before, "nothing written");
    }
}
