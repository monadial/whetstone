//! Connected code: the callers and callees of the callables a change adds
//! or modifies, one hop away, found on the learner's machine with the
//! embedded `cgg` engine ([ADR 0002]). This module is the only one that
//! knows about `cgg`, so the engine can be upgraded or replaced behind it.
//!
//! [`find`] is the whole job for one change, and blocks while it runs, so
//! callers run it off the UI thread. It reads the commit through a
//! [`Snapshot`], writes the files the engine reads into a private
//! [`AnalysisDir`], analyses them there, removes the directory, and
//! returns the connections, with the whole [`Graph`] so it can be cached
//! and asked about the change again.
//!
//! A [`Connection`] is a line range only. Its sent text is the numbered
//! lines of that range in the file at the commit, which the caller reads
//! through the same snapshot, so what is shown and what is sent come from
//! one place.
//!
//! The log records counts, sizes, timings and reasons, never file
//! contents, callable names or the graph.
//!
//! [ADR 0002]: https://github.com/monadial/whetstone/blob/main/docs/adr/0002-embed-cgg-for-connected-code.md

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired into the outbound review by the #58 integration"
    )
)]

mod analysis;
mod detect;
mod graph;
mod lines;

use std::error::Error;
use std::io;
use std::path::Path;
use std::time::Instant;

use whetstone_core::{Change, ExcludePatterns};

use crate::repository::{CommitTree, LoadError, TreeEntryKind};

pub(crate) use analysis::{AnalysisDir, sweep};
pub(crate) use graph::{Connection, Graph};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "named by the outbound review once the #58 integration wires it in"
    )
)]
pub(crate) use graph::{Relation, Tier};

/// A commit's whole tree as finding connected code reads it: every file's
/// path and kind, and any regular file's contents by path. It is a view of
/// the commit, never of a working tree. A repository's commit is one
/// ([`CommitTree`]); the showcase project can make one by replaying its
/// changes up to the one in question.
pub(crate) trait Snapshot {
    /// Why the tree could not be read.
    type Error: Error + Send + Sync + 'static;

    /// Every file in the tree, in any order. Directories are not files.
    fn files(&self) -> Result<Vec<SnapshotFile>, Self::Error>;

    /// The contents of the regular file at `path`, relative to the
    /// repository's root, or `None` if there is no regular file there.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, Self::Error>;
}

/// One file in a [`Snapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotFile {
    path: String,
    kind: FileKind,
}

/// What a file in a [`Snapshot`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    /// A regular file.
    Blob,
    /// A regular file with its executable bit set.
    Executable,
    /// A symbolic link, which is never read.
    Symlink,
    /// A submodule, which is never read.
    Submodule,
}

impl SnapshotFile {
    /// The file at `path`, relative to the repository's root with `/`
    /// between directories, which is of `kind`.
    pub(crate) fn new(path: String, kind: FileKind) -> Self {
        Self { path, kind }
    }

    /// Where the file is, relative to the repository's root.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// What the file is.
    pub(crate) fn kind(&self) -> FileKind {
        self.kind
    }
}

impl Snapshot for CommitTree<'_> {
    type Error = LoadError;

    fn files(&self) -> Result<Vec<SnapshotFile>, LoadError> {
        Ok(self
            .entries()
            .iter()
            .map(|entry| {
                let kind = match entry.kind() {
                    TreeEntryKind::Blob => FileKind::Blob,
                    TreeEntryKind::Executable => FileKind::Executable,
                    TreeEntryKind::Symlink => FileKind::Symlink,
                    TreeEntryKind::Submodule => FileKind::Submodule,
                };
                SnapshotFile::new(entry.path().to_owned(), kind)
            })
            .collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, LoadError> {
        CommitTree::read(self, path)
    }
}

/// Why connected code could not be found.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Failure {
    /// The commit's tree could not be read.
    #[error("cannot read the commit's files")]
    Snapshot(#[source] Box<dyn Error + Send + Sync>),
    /// The system temporary directory's path has a directory the engine
    /// never reads in, such as `build`, so it would find nothing there.
    #[error("the temporary directory is inside a {0:?} directory, which the engine never reads")]
    UnreadableTemporaryDirectory(&'static str),
    /// The analysis directory could not be created or written.
    #[error("cannot write the commit's files for analysis")]
    Write(#[source] io::Error),
    /// The engine failed.
    #[error("the engine failed to analyse the commit's files")]
    Engine(#[source] Box<dyn Error + Send + Sync>),
}

/// What [`find`] found: the change's connected code, and the whole graph
/// it came from.
#[derive(Debug)]
pub(crate) struct Found {
    connections: Vec<Connection>,
    graph: Graph,
}

impl Found {
    /// The callers and callees of the change's callables, in path and
    /// line order. Empty when none were found.
    pub(crate) fn connections(&self) -> &[Connection] {
        &self.connections
    }

    /// The whole call graph of the commit, which can give the connected
    /// code of any change at that commit with [`Graph::connections`].
    pub(crate) fn graph(&self) -> &Graph {
        &self.graph
    }
}

/// Finds the connected code of `change` at its commit, which `snapshot`
/// shows: the callers and callees of the callables it adds or modifies,
/// one hop away, as [`Graph::connections`] describes them.
///
/// It writes the commit's code to a new private directory in `temp_dir`,
/// leaving out what `exclude` and the default exclusions leave out, as
/// [`AnalysisDir::write`] describes, analyses it there, and removes the
/// directory before it returns, whatever the outcome. It blocks until the
/// engine is done, which the engine cannot be made to stop early.
pub(crate) fn find<S: Snapshot>(
    snapshot: &S,
    change: &Change,
    exclude: &ExcludePatterns,
    temp_dir: &Path,
) -> Result<Found, Failure> {
    let started = Instant::now();
    let found = AnalysisDir::write(snapshot, exclude, temp_dir)
        .and_then(|dir| dir.analyse())
        .map(|graph| Found {
            connections: graph.connections(change),
            graph,
        });
    match &found {
        Ok(found) => tracing::info!(
            connections = found.connections.len(),
            ms = started.elapsed().as_millis(),
            "found connected code"
        ),
        Err(failure) => tracing::warn!(
            error = %failure,
            reason = failure.source().map(ToString::to_string),
            "cannot find connected code"
        ),
    }
    found
}

#[cfg(test)]
mod tests;
