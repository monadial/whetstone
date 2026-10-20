//! The private temporary directory a commit's code is written to for the
//! engine, which removes itself, and the sweep at startup for any that a
//! crash left behind.

use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use whetstone_core::{DefaultExclusion, ExcludePatterns, FileContent};

use super::detect::{self, Engine};
use super::{Failure, FileKind, Graph, Snapshot};

/// How every analysis directory's name starts.
const PREFIX: &str = "whetstone-analysis-";

/// How old a leftover analysis directory is before the startup sweep
/// removes it. A younger one may belong to another Whetstone still running.
const LEFTOVER_AGE: Duration = Duration::from_hours(24);

/// An empty `cgg-deadcode.toml` for the engine. Without one, it would look
/// for that file above the analysis directory and above the process's
/// working directory, which is the learner's working tree, so what it
/// found could change the graph or fail the analysis.
const NO_ENGINE_CONFIG: &str = "/dev/null";

/// A commit's files, written to a new private directory for the engine to
/// read: `whetstone-analysis-` and a random suffix, created mode 0700 in
/// the system temporary directory. Dropping it removes the directory and
/// everything in it, including while a panic unwinds.
#[derive(Debug)]
pub(crate) struct AnalysisDir {
    dir: tempfile::TempDir,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired into the outbound review by the #58 integration"
    )
)]
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
    pub(crate) fn write<S: Snapshot>(
        snapshot: &S,
        exclude: &ExcludePatterns,
        temp_dir: &Path,
    ) -> Result<Self, Failure> {
        if let Some(name) = Engine::denied_directory(temp_dir) {
            return Err(Failure::UnreadableTemporaryDirectory(name));
        }
        let started = Instant::now();
        let files = snapshot
            .files()
            .map_err(|error| Failure::Snapshot(Box::new(error)))?;
        let dir = tempfile::Builder::new()
            .prefix(PREFIX)
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in(temp_dir)
            .map_err(Failure::Write)?;
        let engine = Engine::new();
        let mut tally = Tally::default();
        for file in &files {
            let Some(relative) = writable(file.path(), file.kind(), exclude, &mut tally) else {
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
            match write_private(&dir.path().join(relative), text.as_bytes()) {
                Ok(()) => {
                    tally.written += 1;
                    tally.bytes += text.len();
                }
                Err(error) if clashes(&error) => tally.clashes += 1,
                Err(error) => return Err(Failure::Write(error)),
            }
        }
        tally.log(files.len(), started.elapsed());
        Ok(Self { dir })
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
