//! Connected code: the callers and callees of the callables a change adds
//! or modifies, one hop away, found on the learner's machine with the
//! embedded `cgg` engine ([ADR 0002]). This module is the only one that
//! knows about `cgg`, so the engine can be upgraded or replaced behind it.
//!
//! The outbound review asks for a commit's connected code with a
//! [`Request`], which a background job runs. It gives the change's
//! [`Connections`], each with the text it would send, read from the commit,
//! or why there are none.
//!
//! [`find`] is the engine's part of that, for any [`Snapshot`] of a commit,
//! and blocks while it runs, so callers run it off the UI thread. It writes
//! the files the engine reads into a private [`AnalysisDir`], analyses them
//! there, removes the directory, and returns the callers and callees as
//! line ranges, with the whole [`Graph`] so it can be cached and asked about
//! the change again. The app's [`AnalysisDirsGuard`] removes any directory
//! still in use when the app quits.
//!
//! The log records counts, sizes, timings and reasons, never file
//! contents, callable names or the graph.
//!
//! [ADR 0002]: https://github.com/monadial/whetstone/blob/main/docs/adr/0002-embed-cgg-for-connected-code.md

mod analysis;
mod detect;
mod graph;
mod lines;

use std::error::Error;
use std::io;
use std::time::Instant;

use whetstone_core::{
    Change, CommitId, ConnectionGroup, Connections, ExcludePatterns, Extent, FileContent, LineRange,
};

use crate::repository::{CommitTree, LoadError, RepositoryHandle, TreeEntryKind};
use crate::terminal::{self, Panicked};

pub(crate) use analysis::{AnalysisDir, AnalysisDirs, AnalysisDirsGuard, sweep};
pub(crate) use graph::{Connection, Graph, Relation, Tier};

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
    /// Reading the commit or analysing it panicked.
    #[error("finding connected code panicked")]
    Panicked,
    /// The app is quitting, and has removed its analysis directories.
    #[error("the app is quitting")]
    Quitting,
}

/// What [`find`] found: the change's connected code, and the whole graph
/// it came from.
#[derive(Debug)]
pub(crate) struct Found {
    connections: Vec<Connection>,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "kept for the graph cache (#63)")
    )]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "kept for the graph cache (#63)")
    )]
    pub(crate) fn graph(&self) -> &Graph {
        &self.graph
    }
}

/// Finds the connected code of `change` at its commit, which `snapshot`
/// shows: the callers and callees of the callables it adds or modifies,
/// one hop away, as [`Graph::connections`] describes them.
///
/// It writes the commit's code to a new private directory among `dirs`,
/// leaving out what `exclude` and the default exclusions leave out, as
/// [`AnalysisDir::write`] describes, analyses it there, and removes the
/// directory before it returns, whatever the outcome, a panic included,
/// which it reports as [`Failure::Panicked`]. It blocks until the engine is
/// done, which the engine cannot be made to stop early.
pub(crate) fn find<S: Snapshot>(
    snapshot: &S,
    change: &Change,
    exclude: &ExcludePatterns,
    dirs: &AnalysisDirs,
) -> Result<Found, Failure> {
    let started = Instant::now();
    let found = terminal::catch_panic(|| {
        AnalysisDir::write(snapshot, exclude, dirs)
            .and_then(|dir| dir.analyse())
            .map(|graph| Found {
                connections: graph.connections(change),
                graph,
            })
    })
    .unwrap_or_else(|Panicked| Err(Failure::Panicked));
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

/// What a background job needs to find one commit's connected code: its
/// own handle on the repository, the commit and its change, the
/// repository's exclude patterns, and where to analyse it.
#[derive(Debug)]
pub(crate) struct Request {
    repository: RepositoryHandle,
    commit: CommitId,
    change: Change,
    exclude: ExcludePatterns,
    dirs: AnalysisDirs,
}

impl Request {
    /// A request for the connected code of `change`, the commit `commit`
    /// of `repository`, leaving out what `exclude` leaves out, analysed
    /// among `dirs`.
    pub(crate) fn new(
        repository: RepositoryHandle,
        commit: CommitId,
        change: Change,
        exclude: ExcludePatterns,
        dirs: AnalysisDirs,
    ) -> Self {
        Self {
            repository,
            commit,
            change,
            exclude,
            dirs,
        }
    }

    /// The commit it is for.
    pub(crate) fn commit(&self) -> CommitId {
        self.commit
    }

    /// The commit's connected code: the callers and callees of the
    /// callables its change adds or modifies, as [`find`] finds them, each
    /// with the lines it would send, read from the commit; or none found,
    /// or the analysis failed. It blocks until the engine is done.
    pub(crate) fn connections(&self) -> Connections {
        let started = Instant::now();
        let connections = self
            .repository
            .with_tree(self.commit, |tree| {
                let found = find(tree, &self.change, &self.exclude, &self.dirs).ok()?;
                with_sent_text(tree, found.connections())
            })
            .ok()
            .flatten()
            .map_or(Connections::AnalysisFailed, |connections| {
                if connections.is_empty() {
                    Connections::NoneFound
                } else {
                    Connections::Found(connections)
                }
            });
        tracing::info!(
            commit = self.commit.full(),
            state = match &connections {
                Connections::Found(_) => "found",
                Connections::NoneFound => "none found",
                Connections::AnalysisFailed => "analysis failed",
                Connections::Finding | Connections::TookTooLong => "unexpected",
            },
            ms = started.elapsed().as_millis(),
            "found a commit's connected code"
        );
        connections
    }
}

/// `connections` in the commit `tree` shows, as the evidence takes them:
/// connected code, each with its file's text at the commit, of which it
/// keeps only its lines. `None` if a file cannot be read.
fn with_sent_text(
    tree: &CommitTree<'_>,
    connections: &[Connection],
) -> Option<Vec<whetstone_core::Connection>> {
    connections
        .iter()
        .map(|connection| {
            let content = match tree.read(connection.path()) {
                Ok(Some(bytes)) => FileContent::from_bytes(bytes),
                // The engine read every file it connects from this tree.
                Ok(None) | Err(_) => return None,
            };
            let lines = connection.lines();
            let range = LineRange::new(
                usize::try_from(*lines.start()).ok()?,
                usize::try_from(*lines.end()).ok()?,
            )
            .ok()?;
            Some(whetstone_core::Connection::new(
                connection.path().to_owned(),
                Extent::Lines(range),
                ConnectionGroup::ConnectedCode,
                match connection.tier() {
                    Tier::High => whetstone_core::Tier::High,
                    Tier::Medium => whetstone_core::Tier::Medium,
                },
                connection
                    .relations()
                    .iter()
                    .map(|relation| match relation.clone() {
                        Relation::Calls {
                            callable,
                            touched_file,
                        } => whetstone_core::Relation::Calls {
                            callable,
                            touched_file,
                        },
                        Relation::CalledBy {
                            callable,
                            touched_file,
                        } => whetstone_core::Relation::CalledBy {
                            callable,
                            touched_file,
                        },
                    })
                    .collect(),
                content,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
