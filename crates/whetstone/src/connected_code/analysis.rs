//! The private temporary directory a commit's code is written to for the
//! engine, which removes itself, the guard that removes any still in use
//! when the app quits, and the sweep at startup for any that a crash left
//! behind.

use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use whetstone_core::{DefaultExclusion, ExcludePatterns, FileContent};

use super::detect::{self, Engine};
use super::{Failure, FileKind, Graph, Snapshot, SnapshotFile};

/// How every analysis directory's name starts.
const PREFIX: &str = "whetstone-analysis-";

/// How old a leftover analysis directory is before the startup sweep
/// removes it. A younger one may belong to another Whetstone still running.
const LEFTOVER_AGE: Duration = Duration::from_hours(24);

/// How many threads create files at once. Creating files, not reading
/// them, takes most of the time spent writing a large commit, and file
/// systems create files in parallel well.
const WRITERS: usize = 4;

/// How many files wait for a writer at most, which bounds how much of the
/// commit is held in memory at once.
const QUEUED: usize = 256;

/// An empty `cgg-deadcode.toml` for the engine. Without one, it would look
/// for that file above the analysis directory and above the process's
/// working directory, which is the learner's working tree, so what it
/// found could change the graph or fail the analysis.
const NO_ENGINE_CONFIG: &str = "/dev/null";

/// Where analysis directories are made, the system temporary directory,
/// and which of them are in use. Its clones share that record, so a
/// background job can make its directory while the app holds the
/// [`AnalysisDirsGuard`] that removes it when the app quits.
#[derive(Debug, Clone)]
pub(crate) struct AnalysisDirs {
    temp_dir: PathBuf,
    in_use: Arc<RwLock<InUse>>,
}

/// The analysis directories in use, and whether the app is quitting.
#[derive(Debug, Default)]
struct InUse {
    /// Set when the guard drops. No directory is made or written after it.
    closed: bool,
    dirs: Vec<PathBuf>,
}

/// Removes every analysis directory still in use when it drops, as the app
/// quits or a panic unwinds on its thread, and from then on stops any job
/// from making a directory or writing a file. Quitting never waits for a
/// job, so a job's own [`AnalysisDir`] may never drop: the process can exit
/// while it writes or the engine reads.
#[derive(Debug)]
pub(crate) struct AnalysisDirsGuard(Arc<RwLock<InUse>>);

impl AnalysisDirs {
    /// Analysis directories in `temp_dir`, the system temporary directory.
    pub(crate) fn new(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            in_use: Arc::default(),
        }
    }

    /// The guard that removes the directories in use when it drops. Until
    /// one drops, jobs make and write them freely.
    pub(crate) fn guard(&self) -> AnalysisDirsGuard {
        AnalysisDirsGuard(Arc::clone(&self.in_use))
    }

    /// The record, shared, as writing a file into a directory in use takes
    /// it.
    fn read(&self) -> RwLockReadGuard<'_, InUse> {
        self.in_use.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The record, to change.
    fn write(&self) -> RwLockWriteGuard<'_, InUse> {
        write(&self.in_use)
    }
}

/// The record of directories in use, to change, even after a panic while
/// it was held: it is only ever left whole.
fn write(in_use: &RwLock<InUse>) -> RwLockWriteGuard<'_, InUse> {
    in_use.write().unwrap_or_else(PoisonError::into_inner)
}

impl Drop for AnalysisDirsGuard {
    fn drop(&mut self) {
        let mut in_use = write(&self.0);
        in_use.closed = true;
        let (mut removed, mut failed) = (0_usize, 0_usize);
        for dir in in_use.dirs.drain(..) {
            match fs::remove_dir_all(&dir) {
                Ok(()) => removed += 1,
                Err(_) => failed += 1,
            }
        }
        if removed + failed > 0 {
            tracing::info!(removed, failed, "removed the analysis directories in use");
        }
    }
}

/// A commit's files, written to a new private directory for the engine to
/// read: `whetstone-analysis-` and a random suffix, created mode 0700 in
/// the system temporary directory. Dropping it removes the directory and
/// everything in it, including while a panic unwinds, and so does dropping
/// the [`AnalysisDirsGuard`] while it is in use.
#[derive(Debug)]
pub(crate) struct AnalysisDir {
    dir: tempfile::TempDir,
    dirs: AnalysisDirs,
}

impl AnalysisDir {
    /// Writes the files of `snapshot` that the engine reads into a new
    /// analysis directory in `temp_dir`, each at its path in the
    /// repository.
    ///
    /// A file is written only if the engine would read it: its language is
    /// one the engine's detector recognises, it is text no larger than the
    /// engine's limit, and it is not in a directory, such as `target` or
    /// `node_modules`, that the engine never reads in. Symbolic links,
    /// submodules, binary files, lockfiles, secret paths and files the
    /// repository's `exclude` patterns leave out are never written, so a
    /// secret path is never copied anywhere.
    ///
    /// Fails before creating anything if `temp_dir`'s own path has a
    /// directory the engine never reads in, which would hide every file.
    ///
    /// Once the [`AnalysisDirsGuard`] has dropped, it makes nothing, or
    /// stops before its next file, and fails.
    pub(crate) fn write<S: Snapshot>(
        snapshot: &S,
        exclude: &ExcludePatterns,
        dirs: &AnalysisDirs,
    ) -> Result<Self, Failure> {
        if let Some(name) = Engine::denied_directory(&dirs.temp_dir) {
            return Err(Failure::UnreadableTemporaryDirectory(name));
        }
        let started = Instant::now();
        let files = snapshot
            .files()
            .map_err(|error| Failure::Snapshot(Box::new(error)))?;
        let dir = Self::make(dirs)?;
        let engine = Engine::new();
        let mut tally = Tally::default();
        let (sender, queue) = mpsc::sync_channel::<(PathBuf, String)>(QUEUED);
        let dir_ref = &dir;
        let queue = Mutex::new(queue);
        let failed = AtomicBool::new(false);
        let (read, written) = thread::scope(|scope| {
            let writers: Vec<_> = (0..WRITERS)
                .map(|_| scope.spawn(|| write_queued(&queue, &failed, dir_ref)))
                .collect();
            let read = select(
                snapshot,
                &files,
                exclude,
                &engine,
                &mut tally,
                |relative, text| {
                    !failed.load(Ordering::Relaxed) && sender.send((relative, text)).is_ok()
                },
            );
            drop(sender);
            let written: Vec<Written> = writers
                .into_iter()
                .map(|writer| {
                    writer
                        .join()
                        .unwrap_or_else(|panic| panic::resume_unwind(panic))
                })
                .collect();
            (read, written)
        });
        read?;
        for written in written {
            tally.written += written.files;
            tally.bytes += written.bytes;
            tally.clashes += written.clashes;
            if let Some(failure) = written.failure {
                return Err(failure);
            }
        }
        tally.log(files.len(), started.elapsed());
        Ok(dir)
    }

    /// A new, empty analysis directory, recorded as in use.
    fn make(dirs: &AnalysisDirs) -> Result<Self, Failure> {
        let mut in_use = dirs.write();
        if in_use.closed {
            return Err(Failure::Quitting);
        }
        let dir = tempfile::Builder::new()
            .prefix(PREFIX)
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in(&dirs.temp_dir)
            .map_err(Failure::Write)?;
        in_use.dirs.push(dir.path().to_path_buf());
        Ok(Self {
            dir,
            dirs: dirs.clone(),
        })
    }

    /// Writes `contents` to `relative` inside the directory, unless the
    /// [`AnalysisDirsGuard`] has dropped. Writers share the record while
    /// they write, and the guard takes it whole, so it never removes the
    /// directory while a file is being written into it, which would make it
    /// again.
    fn write_file(&self, relative: &Path, contents: &[u8]) -> Result<io::Result<()>, Failure> {
        let in_use = self.dirs.read();
        if in_use.closed {
            return Err(Failure::Quitting);
        }
        Ok(write_private(&self.path().join(relative), contents))
    }

    /// The analysis directory.
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs the engine over the directory, with no filter, no hop limit
    /// and no Git, and returns the whole call graph it finds.
    pub(crate) fn analyse(&self) -> Result<Graph, Failure> {
        let started = Instant::now();
        let options = cgg::RunOptions {
            paths: vec![self.path().to_path_buf()],
            roots: Some(PathBuf::from(NO_ENGINE_CONFIG)),
            ..cgg::RunOptions::default()
        };
        let outcome = cgg::analyze(&options).map_err(|error| Failure::Engine(error.into()))?;
        let graph = Graph::found_in(outcome.graph, self.path());
        let (files, callables, calls) = graph.sizes();
        tracing::info!(
            files,
            callables,
            calls,
            threads = outcome.jobs,
            ms = started.elapsed().as_millis(),
            "analysed a commit's files"
        );
        Ok(graph)
    }
}

impl Drop for AnalysisDir {
    /// Removes the directory while the record is locked, so that the app
    /// never exits with it half removed. Its `TempDir` then finds nothing
    /// left to remove.
    fn drop(&mut self) {
        let mut in_use = self.dirs.write();
        in_use.dirs.retain(|dir| dir != self.dir.path());
        if let Err(error) = fs::remove_dir_all(self.dir.path())
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(error = %error, "cannot remove an analysis directory");
        }
    }
}

/// How many of a commit's files were written for the engine, and why the
/// rest were not.
#[derive(Debug, Default)]
struct Tally {
    written: usize,
    bytes: usize,
    symlinks: usize,
    submodules: usize,
    unsafe_paths: usize,
    default_exclusions: usize,
    repository_exclusions: usize,
    never_read: usize,
    too_large: usize,
    binary: usize,
    not_code: usize,
    clashes: usize,
}

impl Tally {
    /// Records the counts in the log, never a path.
    fn log(&self, files: usize, elapsed: Duration) {
        tracing::info!(
            files,
            written = self.written,
            bytes = self.bytes,
            symlinks = self.symlinks,
            submodules = self.submodules,
            unsafe_paths = self.unsafe_paths,
            default_exclusions = self.default_exclusions,
            repository_exclusions = self.repository_exclusions,
            never_read = self.never_read,
            too_large = self.too_large,
            binary = self.binary,
            not_code = self.not_code,
            clashes = self.clashes,
            ms = elapsed.as_millis(),
            "wrote a commit's files for analysis"
        );
    }
}

/// Reads the files of `snapshot` among `files` that the engine reads, and
/// passes each to `write` with where it goes in the analysis directory,
/// until `write` says to stop. The rest are counted in `tally`.
fn select<S: Snapshot>(
    snapshot: &S,
    files: &[SnapshotFile],
    exclude: &ExcludePatterns,
    engine: &Engine,
    tally: &mut Tally,
    mut write: impl FnMut(PathBuf, String) -> bool,
) -> Result<(), Failure> {
    for file in files {
        let Some(relative) = writable(file.path(), file.kind(), exclude, tally) else {
            continue;
        };
        let contents = snapshot
            .read(file.path())
            .map_err(|error| Failure::Snapshot(Box::new(error)))?
            .unwrap_or_default();
        if !engine.reads_size(contents.len()) {
            tally.too_large += 1;
            continue;
        }
        let FileContent::Text(text) = FileContent::from_bytes(contents) else {
            tally.binary += 1;
            continue;
        };
        if !engine.recognises(file.path(), text.as_bytes()) {
            tally.not_code += 1;
            continue;
        }
        if !write(relative, text) {
            break;
        }
    }
    Ok(())
}

/// What one writer wrote.
#[derive(Debug, Default)]
struct Written {
    files: usize,
    bytes: usize,
    clashes: usize,
    /// Why the first write that failed for a reason other than a clash
    /// failed, or that the app is quitting.
    failure: Option<Failure>,
}

/// Writes each file taken from `queue` into `dir` until it is empty and
/// closed. Once a write fails, or the app quits, it sets `failed` and keeps
/// taking files without writing them, so the reader never waits on a full
/// queue.
fn write_queued(
    queue: &Mutex<Receiver<(PathBuf, String)>>,
    failed: &AtomicBool,
    dir: &AnalysisDir,
) -> Written {
    let mut written = Written::default();
    loop {
        let next = queue
            .lock()
            .expect("no writer panics while taking a file")
            .recv();
        let Ok((relative, text)) = next else {
            return written;
        };
        if written.failure.is_some() {
            continue;
        }
        match dir.write_file(&relative, text.as_bytes()) {
            Ok(Ok(())) => {
                written.files += 1;
                written.bytes += text.len();
            }
            Ok(Err(error)) if clashes(&error) => written.clashes += 1,
            Ok(Err(error)) => {
                failed.store(true, Ordering::Relaxed);
                written.failure = Some(Failure::Write(error));
            }
            Err(failure) => {
                failed.store(true, Ordering::Relaxed);
                written.failure = Some(failure);
            }
        }
    }
}

/// Where the file at `path` of `kind` is written in the analysis
/// directory, or `None`, counted in `tally`, if its path alone keeps it
/// out.
fn writable(
    path: &str,
    kind: FileKind,
    exclude: &ExcludePatterns,
    tally: &mut Tally,
) -> Option<PathBuf> {
    let counter = match kind {
        FileKind::Symlink => &mut tally.symlinks,
        FileKind::Submodule => &mut tally.submodules,
        FileKind::Blob | FileKind::Executable => {
            let Some(relative) = detect::relative_path(path) else {
                tally.unsafe_paths += 1;
                return None;
            };
            if DefaultExclusion::of(path).is_some() {
                &mut tally.default_exclusions
            } else if exclude.first_match(path).is_some() {
                &mut tally.repository_exclusions
            } else if Engine::denied_directory(&relative).is_some() {
                &mut tally.never_read
            } else {
                return Some(relative);
            }
        }
    };
    *counter += 1;
    None
}

/// Writes `contents` to a new file at `path`, readable only by its owner,
/// creating the directories above it.
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(contents)
}

/// Whether writing a file failed because another file or directory is
/// already where it goes, as two paths that differ only in case are on a
/// case-insensitive file system, or because the file system cannot name it.
/// The file is then skipped rather than failing the analysis.
fn clashes(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AlreadyExists
            | io::ErrorKind::NotADirectory
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::InvalidFilename
    )
}

/// Removes the analysis directories in `temp_dir` last changed more than a
/// day before `now`, which a crash, or quitting during an analysis, can
/// leave behind. Anything else, and anything it cannot remove, such as
/// another user's, is left alone.
pub(crate) fn sweep(temp_dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(temp_dir) else {
        return;
    };
    let (mut removed, mut failed) = (0_usize, 0_usize);
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(PREFIX));
        // The entry's own metadata: a symbolic link is never followed.
        let stale = entry.metadata().is_ok_and(|metadata| {
            metadata.is_dir()
                && metadata
                    .modified()
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > LEFTOVER_AGE)
        });
        if !(ours && stale) {
            continue;
        }
        match fs::remove_dir_all(entry.path()) {
            Ok(()) => removed += 1,
            Err(_) => failed += 1,
        }
    }
    if removed + failed > 0 {
        tracing::info!(removed, failed, "removed leftover analysis directories");
    }
}
