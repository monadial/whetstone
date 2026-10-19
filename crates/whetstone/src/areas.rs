//! `whetstone areas init`: copies Whetstone's default area files into the
//! repository it runs in, the only time Whetstone writes into a
//! repository.

use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use whetstone_core::Area;

use crate::repository::{self, Discovery};

/// Why `whetstone areas init` wrote nothing, or stopped part way.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InitError {
    /// It did not run inside a Git repository.
    #[error(
        "not inside a Git repository: run `whetstone areas init` inside the repository the areas are for"
    )]
    NotInRepository,
    /// The repository is bare, so it has no working tree for area files.
    #[error("{} is a bare repository, with no working tree for area files", .0.display())]
    Bare(PathBuf),
    /// A `.git` was found that cannot be opened as a repository.
    #[error("{} has a .git that cannot be opened as a repository", .0.display())]
    Unusable(PathBuf),
    /// The repository cannot be read.
    #[error("cannot read the repository at {}", .path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: repository::DiscoveryError,
    },
    /// The areas directory already holds area files.
    #[error("{} already has area files, so nothing was written", .0.display())]
    HasAreaFiles(PathBuf),
    /// The areas directory cannot be created or read.
    #[error("cannot use the areas directory {}", .path.display())]
    Directory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// An area file cannot be written.
    #[error("cannot write the area file {}", .path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Finds the Git root from `start`, creates `.whetstone/areas/` there if
/// needed, and writes the default area files into it. Returns the path of
/// each file written, in file-name order. If the directory already holds
/// any `.md` file, it writes nothing.
pub(crate) fn init(start: &Path) -> Result<Vec<PathBuf>, InitError> {
    let root = match repository::discover(start) {
        Ok(Discovery::WorkTree(repository)) => repository::root_of(&repository).to_path_buf(),
        Ok(Discovery::Bare(path)) => return Err(InitError::Bare(path)),
        Ok(Discovery::Unusable(dir)) => return Err(InitError::Unusable(dir)),
        Ok(Discovery::NotInRepository) => return Err(InitError::NotInRepository),
        Err(source) => {
            return Err(InitError::Unreadable {
                path: start.to_path_buf(),
                source,
            });
        }
    };
    let dir = root.join(repository::AREAS);
    let directory = |source| InitError::Directory {
        path: dir.clone(),
        source,
    };
    fs::create_dir_all(&dir).map_err(directory)?;
    for entry in fs::read_dir(&dir).map_err(directory)? {
        let entry = entry.map_err(directory)?;
        if Path::new(&entry.file_name()).extension() == Some(OsStr::new("md")) {
            return Err(InitError::HasAreaFiles(dir));
        }
    }
    let mut written = Vec::new();
    for &(file_name, contents) in Area::default_files() {
        let path = dir.join(file_name);
        // Never overwrite a file, even one that appeared since the check.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| file.write_all(contents))
            .map_err(|source| InitError::Write {
                path: path.clone(),
                source,
            })?;
        written.push(path);
    }
    Ok(written)
}
