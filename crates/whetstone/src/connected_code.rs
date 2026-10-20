//! Connected code: the callers and callees of the callables a change adds
//! or modifies, one hop away, found on the learner's machine with the
//! embedded `cgg` engine ([ADR 0002]), and the repository files the change
//! links to in Markdown and names on the lines it adds or modifies. This
//! module is the only one that knows about `cgg`, so the engine can be
//! upgraded or replaced behind it.
//!
//! The outbound review asks for a commit's connected code with a
//! [`Request`], which a background job runs. It gives the change's
//! [`Connections`], each with the text it would send, read from the commit,
//! or why there are none, and whether they were analysed now or came from
//! the cache. They include the change's related tests and related
//! configuration, found from the commit's paths by convention. A showcase
//! change's are found the same way, from its [`ShowcaseTree`], which
//! replays the showcase project's embedded changes up to it.
//!
//! [`find`] is the engine's part of that, for any [`Snapshot`] of a commit,
//! and blocks while it runs, so callers run it off the UI thread. It keeps
//! each tree's whole [`Graph`] in a [`GraphCache`], so asking about a tree
//! analysed before writes and analyses nothing. Otherwise it writes the
//! files the engine reads into a private [`AnalysisDir`], analyses them
//! there, removes the directory and caches the graph. Either way it returns
//! the callers and callees as line ranges. The app's [`AnalysisDirsGuard`]
//! removes any directory still in use when the app quits.
//!
//! The log records counts, sizes, timings and reasons, never file
//! contents, callable names or the graph.
//!
//! [ADR 0002]: https://github.com/monadial/whetstone/blob/main/docs/adr/0002-embed-cgg-for-connected-code.md
//! [`ShowcaseTree`]: showcase::ShowcaseTree

mod analysis;
mod cache;
mod detect;
mod graph;
mod lines;
mod linked_and_named;
mod links;
mod mentions;
mod related;
mod showcase;
mod touched_text;

use std::error::Error;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

use whetstone_core::{
    Change, CommitId, ConnectionGroup, Connections, ExcludePatterns, Extent, FileContent,
    LineRange, merge_into_whole_files,
};

use crate::repository::{CommitTree, LoadError, RepositoryHandle, TreeEntryKind};

pub(crate) use analysis::{AnalysisDir, AnalysisDirs, AnalysisDirsGuard, sweep};
pub(crate) use cache::{GraphCache, SnapshotId};
pub(crate) use graph::{Connection, Graph, Relation, Tier};

/// How long the outbound review waits for a commit's connected code before
/// it carries on without it, counted from when the review opens: any wait
/// for another commit's analysis, the cache lookup, writing the commit's
/// files and analysing them.
///
/// Measured on an Apple M4 Max, a release build finds it for tokio (565
/// files) in about 0.4 s, and for rust-lang/rust (63,812 files) in about
/// 28 s: 25 s to write, analyse and remove its files, of which 21 s is the
/// analysis, then 1.6 s to cache the graph. A cached graph loads in under
/// 1 s for either. So the limit only gives up on very large repositories,
/// or large ones on slower machines, and the next review of that commit
/// takes its connected code from the cache.
pub(crate) const TIME_LIMIT: Duration = Duration::from_secs(20);

/// A commit's whole tree as finding connected code reads it: every file's
/// path and kind, and any regular file's contents by path. It is a view of
/// the commit, never of a working tree. A repository's commit is one
/// ([`CommitTree`]); the showcase project can make one by replaying its
/// changes up to the one in question.
pub(crate) trait Snapshot {
    /// Why the tree could not be read.
    type Error: Error + Send + Sync + 'static;

    /// Which tree it is, which names its graph in the cache.
    fn id(&self) -> SnapshotId;

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

    fn id(&self) -> SnapshotId {
        SnapshotId::Tree(CommitTree::id(self))
    }

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
    /// Connected code was found in a file that is not a regular file in
    /// the commit.
    #[error("connected code was found in a file that is not in the commit")]
    NotInCommit,
    /// The app is quitting, and has removed its analysis directories.
    #[error("the app is quitting")]
    Quitting,
}

/// What [`find`] found: the change's connected code, the whole graph it
/// came from, and how that graph was found.
#[derive(Debug)]
pub(crate) struct Found {
    connections: Vec<Connection>,
    graph: Graph,
    origin: GraphOrigin,
}

/// How a commit's graph was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphOrigin {
    /// The engine analysed the commit's files.
    Analysed,
    /// An earlier analysis of the same tree was in the cache, so nothing
    /// was written or analysed: the review marks it "from cache".
    Cached,
}

impl Found {
    /// The connected code of `change` in `graph`, which was found as
    /// `origin` says.
    fn new(graph: Graph, change: &Change, origin: GraphOrigin) -> Self {
        Self {
            connections: graph.connections(change),
            graph,
            origin,
        }
    }

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

    /// Whether the graph was analysed now or came from the cache.
    pub(crate) fn origin(&self) -> GraphOrigin {
        self.origin
    }
}

/// Finds the connected code of `change` at its commit, which `snapshot`
/// shows: the callers and callees of the callables it adds or modifies,
/// one hop away, as [`Graph::connections`] describes them.
///
/// If `cache` holds the graph of the snapshot's tree, found by this
/// version of the engine, the connected code comes from it, and nothing is
/// written or analysed. Otherwise it writes the commit's code to a new
/// private directory among `dirs`, leaving out what `exclude` and the
/// default exclusions leave out, as [`AnalysisDir::write`] describes,
/// analyses it there, removes the directory, whatever the outcome, and
/// caches the graph. A panic is reported as [`Failure::Panicked`]. It
/// blocks until the engine is done, which the engine cannot be made to
/// stop early.
pub(crate) fn find<S: Snapshot>(
    snapshot: &S,
    change: &Change,
    exclude: &ExcludePatterns,
    dirs: &AnalysisDirs,
    cache: &GraphCache,
) -> Result<Found, Failure> {
    let started = Instant::now();
    let found = catching(|| {
        let key = cache::GraphKey::of(snapshot.id(), analysis::ENGINE_VERSION);
        if let Some(graph) = cache.get::<Graph>(&key) {
            return Ok(Found::new(graph, change, GraphOrigin::Cached));
        }
        // The directory is removed at the end of this statement, before
        // the graph is cached.
        let graph = AnalysisDir::write(snapshot, exclude, dirs)?.analyse()?;
        let found = Found::new(graph, change, GraphOrigin::Analysed);
        cache.put(&key, found.graph());
        Ok(found)
    });
    match &found {
        Ok(found) => tracing::info!(
            connections = found.connections.len(),
            origin = ?found.origin,
            ms = started.elapsed().as_millis(),
            "found connected code"
        ),
        Err(failure) => tracing::warn!(
            error = %failure,
            // Only an I/O error's kind: the engine's and Git's own errors
            // can name the learner's files.
            io = match failure {
                Failure::Write(error) => Some(error.kind().to_string()),
                _ => None,
            },
            "cannot find connected code"
        ),
    }
    found
}

/// What `work` returns, or [`Failure::Panicked`] once a panic in it has
/// unwound, dropping what it held, such as an analysis directory. The
/// terminal guard's panic hook leaves the terminal alone for a panic off
/// the UI thread, so the app carries on.
fn catching<T>(work: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    panic::catch_unwind(AssertUnwindSafe(work)).unwrap_or(Err(Failure::Panicked))
}

/// What a background job needs to find one commit's connected code: its
/// own handle on the repository, the commit and its change, the
/// repository's exclude patterns, where to analyse it and the cache of
/// graphs analysed before.
#[derive(Debug)]
pub(crate) struct Request {
    repository: RepositoryHandle,
    commit: CommitId,
    change: Change,
    exclude: ExcludePatterns,
    dirs: AnalysisDirs,
    cache: Arc<GraphCache>,
}

impl Request {
    /// A request for the connected code of `change`, the commit `commit`
    /// of `repository`, leaving out what `exclude` leaves out, analysed
    /// among `dirs` unless `cache` holds its graph.
    pub(crate) fn new(
        repository: RepositoryHandle,
        commit: CommitId,
        change: Change,
        exclude: ExcludePatterns,
        dirs: AnalysisDirs,
        cache: Arc<GraphCache>,
    ) -> Self {
        Self {
            repository,
            commit,
            change,
            exclude,
            dirs,
            cache,
        }
    }

    /// The commit it is for.
    pub(crate) fn commit(&self) -> CommitId {
        self.commit
    }

    /// Finds the commit's connected code, blocking until it is found, as
    /// [`connections_at`] finds it in the commit's tree; or the analysis
    /// failed, because the tree could not be read or reading it panicked.
    pub(crate) fn find(&self) -> (Connections, GraphOrigin) {
        let started = Instant::now();
        let (connections, origin) = catching(|| {
            self.repository
                .with_tree(self.commit, |tree| {
                    connections_at(tree, &self.change, &self.exclude, &self.dirs, &self.cache)
                })
                .map_err(|error| Failure::Snapshot(Box::new(error)))
        })
        .unwrap_or((Connections::AnalysisFailed, GraphOrigin::Analysed));
        tracing::info!(
            commit = self.commit.full(),
            state = log_state(&connections),
            origin = ?origin,
            ms = started.elapsed().as_millis(),
            "found a commit's connected code"
        );
        (connections, origin)
    }
}

/// The connected code of `change` at the commit `snapshot` shows, as the
/// evidence takes it, blocking until it is found: the callers and callees
/// of the callables the change adds or modifies, as [`find`] finds them in
/// a cached graph or the engine's, each with the lines it would send, the
/// files the change links to and names, each whole, as
/// [`with_linked_and_named`](linked_and_named::with_linked_and_named) finds
/// them, and its related tests and related configuration, each whole, as
/// [`related::connections`] finds them, with any range in a file sent whole
/// merged into it; or none found, or the analysis failed, a panic included.
/// Everything but the graph is read from the snapshot, so links, names,
/// related tests and related configuration are found whether or not the
/// graph came from the cache. It also says how the graph was found, which
/// is [`GraphOrigin::Analysed`] if none was.
///
/// A learner's commit and a showcase change both find their connected code
/// this way, so the showcase project's is what a learner's repository
/// would give.
fn connections_at<S: Snapshot>(
    snapshot: &S,
    change: &Change,
    exclude: &ExcludePatterns,
    dirs: &AnalysisDirs,
    cache: &GraphCache,
) -> (Connections, GraphOrigin) {
    let mut origin = GraphOrigin::Analysed;
    let found = catching(|| {
        let found = find(snapshot, change, exclude, dirs, cache)?;
        origin = found.origin();
        let code = with_sent_text(snapshot, found.connections())?;
        let mut connections = linked_and_named::with_linked_and_named(snapshot, change, code)?;
        connections.extend(related::connections(snapshot, change)?);
        Ok(merge_into_whole_files(connections))
    });
    let connections = match found {
        Ok(connections) if connections.is_empty() => Connections::NoneFound,
        Ok(connections) => Connections::Found(connections),
        Err(_) => Connections::AnalysisFailed,
    };
    (connections, origin)
}

/// What the log records of `connections`: whether any were found.
fn log_state(connections: &Connections) -> &'static str {
    match connections {
        Connections::Found(_) => "found",
        Connections::NoneFound => "none found",
        Connections::Finding | Connections::TookTooLong | Connections::AnalysisFailed => {
            "analysis failed"
        }
    }
}

/// `connections` in the commit `snapshot` shows, as the evidence takes
/// them: connected code, each with its file's text at the commit, of which
/// it keeps only its lines. It fails if a file cannot be read.
fn with_sent_text<S: Snapshot>(
    snapshot: &S,
    connections: &[Connection],
) -> Result<Vec<whetstone_core::Connection>, Failure> {
    connections
        .iter()
        .map(|connection| {
            let content = read_file(snapshot, connection.path())?;
            let lines = connection.lines();
            let line = |number: u32| usize::try_from(number).expect("a line number fits in usize");
            let range = LineRange::new(line(*lines.start()), line(*lines.end()))
                .expect("the engine's ranges start at line 1 and never end before they start");
            Ok(whetstone_core::Connection::new(
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
        .collect::<Result<_, _>>()
        .inspect_err(|failure| {
            tracing::warn!(error = %failure, "cannot read the files of a change's connected code");
        })
}

/// The contents of the regular file at `path` in `snapshot`, as the
/// evidence takes them. It fails if the file cannot be read, or is not a
/// regular file there.
fn read_file<S: Snapshot>(snapshot: &S, path: &str) -> Result<FileContent, Failure> {
    match snapshot.read(path) {
        Ok(Some(bytes)) => Ok(FileContent::from_bytes(bytes)),
        Ok(None) => Err(Failure::NotInCommit),
        Err(error) => Err(Failure::Snapshot(Box::new(error))),
    }
}

#[cfg(test)]
mod tests;
