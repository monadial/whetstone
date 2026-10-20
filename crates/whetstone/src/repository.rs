//! The learner's repository, found from the directory Whetstone starts in
//! and opened once, at startup, or the reason Practice on it is
//! unavailable.
//!
//! Opening reads `.whetstone/config.toml` and `.whetstone/areas/*.md` from
//! the working tree at the Git root, and never writes anything. From there,
//! [`History`] lists the commits reachable from HEAD a page at a time, and
//! [`Repository::load`] reads one of them as a change from Git's object
//! store. Every `gix` call Whetstone makes is in this module and those it
//! holds, behind Whetstone's own types.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use whetstone_core::{
    Area, Change, CommitId, InvalidAreaFile, RepositoryConfig, RepositoryConfigError,
};

mod history;
mod load;

pub(crate) use history::{Availability, Commit, History, HistoryEnd};

/// The repository's Whetstone config, relative to the Git root.
pub(crate) const CONFIG: &str = ".whetstone/config.toml";
/// The directory of the repository's area files, relative to the Git root.
pub(crate) const AREAS: &str = ".whetstone/areas";

/// The learner's repository as opened at startup.
#[derive(Debug)]
pub struct Repository {
    /// The Git root: the top of the working tree.
    root: PathBuf,
    /// The Git root directory's name, as home shows it.
    name: String,
    /// The areas the repository offers, in file-name order, with the
    /// invalid area files among them.
    areas: Vec<Result<Area, InvalidAreaFile>>,
    /// The repository's Whetstone config.
    config: RepositoryConfig,
    /// The repository, opened read-only.
    git: gix::Repository,
    /// What HEAD pointed to at startup. Commits are listed from here, so
    /// switching branches while Whetstone is open changes nothing.
    head: gix::ObjectId,
}

/// Why Practice on the learner's repository is unavailable.
#[derive(Debug)]
pub enum Unavailable {
    /// Whetstone did not start inside a Git repository.
    NotInRepository,
    /// The repository is bare: it has no working tree to practice in.
    Bare {
        /// The repository's directory name.
        name: String,
    },
    /// The repository has no commits yet.
    NoCommits {
        /// The repository's name.
        name: String,
    },
    /// The repository's Whetstone config is invalid.
    InvalidConfig {
        /// The repository's name.
        name: String,
        problem: RepositoryConfigError,
    },
    /// A Whetstone file in the repository cannot be read.
    Unreadable {
        /// The repository's name.
        name: String,
        /// The file or directory, relative to the Git root.
        path: PathBuf,
        problem: io::Error,
    },
    /// A directory above the start has a `.git` that cannot be opened as a
    /// repository.
    Unusable {
        /// The directory's name.
        name: String,
    },
    /// A repository was found, but it cannot be opened or read.
    Broken(OpenError),
}

/// What looking for a repository from a directory found.
#[derive(Debug)]
pub(crate) enum Discovery {
    /// A repository with a working tree, opened read-only.
    WorkTree(Box<gix::Repository>),
    /// A bare repository, at this path.
    Bare(PathBuf),
    /// A `.git` that is not a repository gix can open, such as one that
    /// cannot be read or a `.git` file pointing at a missing directory, in
    /// this directory, and no repository above it.
    Unusable(PathBuf),
    /// No repository: the directory is not inside one.
    NotInRepository,
}

/// A repository was found but cannot be opened.
#[derive(Debug, thiserror::Error)]
#[error("cannot open the repository")]
pub struct OpenError(#[source] gix::Error);

/// A commit could not be read from the repository.
#[derive(Debug, thiserror::Error)]
#[error("cannot read the commit")]
pub(crate) struct LoadError(#[from] gix::Error);

/// The commits could not be read from the repository.
#[derive(Debug, thiserror::Error)]
#[error("cannot read the commits")]
pub(crate) struct HistoryError(#[from] gix::Error);

/// Looks for the repository that `start` is in, walking up to its Git
/// root, which may be a linked worktree's, and opens it.
///
/// Opening only reads: gix refreshes no index, takes no locks and runs no
/// hooks, filters or `git` processes on the paths Whetstone uses. It is
/// isolated, too: it reads the repository's own config, but no system or
/// global config and no environment variables, so the start directory
/// alone decides what opens, and nothing in the learner's home changes it.
pub(crate) fn discover(start: &Path) -> Result<Discovery, OpenError> {
    use gix::sec::trust::DefaultForLevel as _;

    let isolated = |trust| {
        gix::open::Options::default_for_level(trust).permissions(gix::open::Permissions::isolated())
    };
    let opened = gix::ThreadSafeRepository::discover_opts(
        start,
        gix::discover::upwards::Options::default(),
        gix::sec::trust::Mapping {
            full: isolated(gix::sec::Trust::Full),
            reduced: isolated(gix::sec::Trust::Reduced),
        },
    );
    let mut repository: gix::Repository = match opened {
        Ok(repository) => repository.into(),
        // gix passes over a `.git` it cannot open and looks further up, so
        // finding one on the way says why there is no repository.
        Err(error) if is_not_a_repository(&error) => {
            return Ok(start
                .ancestors()
                .find(|dir| fs::symlink_metadata(dir.join(".git")).is_ok())
                .map_or(Discovery::NotInRepository, |dir| {
                    Discovery::Unusable(dir.to_path_buf())
                }));
        }
        Err(error) => return Err(OpenError(error)),
    };
    if repository.is_bare() {
        return Ok(Discovery::Bare(repository.git_dir().to_path_buf()));
    }
    // Listing commits decodes each one twice: once to sort them by time,
    // and once for its row.
    repository.object_cache_size_if_unset(16 * 1024 * 1024);
    Ok(Discovery::WorkTree(Box::new(repository)))
}

/// Whether `error` says no repository was found, rather than that one was
/// found and is broken.
fn is_not_a_repository(error: &gix::Error) -> bool {
    use gix::discover::upwards::Error;

    matches!(
        error.downcast_any_ref::<Error>(),
        Some(
            Error::NoGitRepository { .. }
                | Error::NoGitRepositoryWithinCeiling { .. }
                | Error::NoGitRepositoryWithinFs { .. }
        )
    )
}

/// The Git root of `repository`: the top of its working tree, which
/// [`discover`] opens repositories only if they have.
pub(crate) fn root_of(repository: &gix::Repository) -> &Path {
    repository
        .workdir()
        .expect("discovery opens only repositories with a working tree")
}

/// The name of the directory at `path`, or the whole path if it has none,
/// as `/` has not.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

impl Repository {
    /// Finds the repository that `start` is in, walking up to its Git root,
    /// and opens it read-only, with the Whetstone config and area files in
    /// its working tree.
    pub fn open(start: &Path) -> Result<Self, Unavailable> {
        let git = match discover(start).map_err(Unavailable::Broken)? {
            Discovery::WorkTree(git) => *git,
            Discovery::Bare(path) => {
                return Err(Unavailable::Bare {
                    name: name_of(&path),
                });
            }
            Discovery::Unusable(dir) => {
                return Err(Unavailable::Unusable {
                    name: name_of(&dir),
                });
            }
            Discovery::NotInRepository => return Err(Unavailable::NotInRepository),
        };
        let root = root_of(&git).to_path_buf();
        // A linked worktree is named after its own directory, as
        // `git rev-parse --show-toplevel` names its root.
        let name = name_of(&root);
        let broken = |error| Unavailable::Broken(OpenError(error));
        // An unborn HEAD has no ID to ask for.
        if git.head().map_err(broken)?.is_unborn() {
            return Err(Unavailable::NoCommits { name });
        }
        let head = git.head_id().map_err(broken)?.detach();
        let unreadable = |path: &str| {
            let name = name.clone();
            let path = PathBuf::from(path);
            move |problem| Unavailable::Unreadable {
                name,
                path,
                problem,
            }
        };
        let config =
            match read_file_if_any(&root.join(CONFIG), usize::MAX).map_err(unreadable(CONFIG))? {
                Some(contents) => RepositoryConfig::parse(&contents).map_err(|problem| {
                    Unavailable::InvalidConfig {
                        name: name.clone(),
                        problem,
                    }
                })?,
                None => RepositoryConfig::default(),
            };
        let area_files = read_area_files(&root.join(AREAS)).map_err(unreadable(AREAS))?;
        let areas = Area::offered(area_files.iter().map(|file| {
            (
                file.file_name.as_str(),
                file.contents.as_deref().map_err(Clone::clone),
            )
        }));
        Ok(Self {
            root,
            name,
            areas,
            config,
            git,
            head,
        })
    }

    /// The Git root directory's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The areas the repository offers, in file-name order, each valid or
    /// the reason its file is not.
    pub(crate) fn areas(&self) -> &[Result<Area, InvalidAreaFile>] {
        &self.areas
    }

    /// The repository's Whetstone config.
    pub(crate) fn config(&self) -> &RepositoryConfig {
        &self.config
    }

    /// The commit `id` as a change, read from Git's object store at that
    /// commit, never from the working tree.
    pub(crate) fn load(&self, id: CommitId) -> Result<Change, LoadError> {
        load::load(&self.git, id).inspect_err(|error| {
            tracing::warn!(commit = id.full(), error = %error.0, "cannot read a commit");
        })
    }

    /// The commits reachable from HEAD as it was at startup, newest first,
    /// in `git log` order, with the first page read.
    pub(crate) fn history(&self) -> Result<History, HistoryError> {
        History::read_from(&self.git, self.head)
    }
}

/// The commit `id`, with a short ID of `short_len` digits.
fn commit_id(id: gix::ObjectId, short_len: usize) -> CommitId {
    CommitId::new(&id.to_string(), short_len).expect("gix writes object IDs as lowercase hex")
}

/// Records in the log how opening the learner's repository went: its Git
/// root, or the kind of reason it is unavailable. Never a file's contents,
/// which a config problem can quote.
pub(crate) fn log(opened: &Result<Repository, Unavailable>) {
    match opened {
        Ok(repository) => tracing::info!(
            root = %repository.root.display(),
            head = %repository.head,
            areas = repository.areas.len(),
            "opened the learner's repository"
        ),
        Err(Unavailable::NotInRepository) => {
            tracing::info!("not started inside a Git repository");
        }
        Err(Unavailable::Bare { name }) => tracing::info!(name, "started in a bare repository"),
        Err(Unavailable::NoCommits { .. }) => tracing::info!("the repository has no commits"),
        Err(Unavailable::Unusable { name }) => {
            tracing::info!(name, "found a .git that cannot be opened");
        }
        Err(Unavailable::InvalidConfig { .. }) => {
            tracing::info!(
                path = CONFIG,
                "the repository's Whetstone config is invalid"
            );
        }
        Err(Unavailable::Unreadable { path, problem, .. }) => tracing::info!(
            path = %path.display(),
            error = %problem,
            "cannot read a Whetstone file in the repository"
        ),
        Err(Unavailable::Broken(error)) => {
            tracing::warn!(error = %error.0, "cannot open the repository");
        }
    }
}

/// The contents of the regular file at `path`, or `None` if there is no
/// file there, as [`read_regular_file`] reads it.
fn read_file_if_any(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
    match read_regular_file(path, limit) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The contents of the regular file at `path`, following symlinks, or of as
/// much of it as `limit` bytes and one more, so a reader can tell it is
/// larger. Anything else at `path`, such as a directory or a named pipe, is
/// an error rather than read.
fn read_regular_file(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::other("it is not a file"));
    }
    let mut contents = Vec::new();
    File::open(path)?
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut contents)?;
    Ok(contents)
}

/// Whether `file_name` is an area file's: one that ends in `.md`, in any
/// case, so that `Security.MD` is listed as an invalid area file rather
/// than passed over.
pub(crate) fn is_area_file_name(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// An entry of `.whetstone/areas/` as read from the working tree.
#[derive(Debug)]
struct AreaFile {
    file_name: String,
    /// The file's contents, or why they could not be read.
    contents: Result<Vec<u8>, String>,
}

/// The area files in `dir`: every entry whose name ends in `.md`, with its
/// contents, or why they could not be read, as for a symlink to nothing or
/// a directory. Empty if there is no such directory.
fn read_area_files(dir: &Path) -> io::Result<Vec<AreaFile>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(file_name) = path.file_name().map(OsStr::to_string_lossy) else {
            continue;
        };
        if !is_area_file_name(&file_name) {
            continue;
        }
        let contents =
            read_regular_file(&path, Area::FILE_LIMIT).map_err(|error| error.to_string());
        files.push(AreaFile {
            file_name: file_name.into_owned(),
            contents,
        });
    }
    Ok(files)
}
