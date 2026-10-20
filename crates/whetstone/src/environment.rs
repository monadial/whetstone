//! The environment Whetstone runs in, captured once and passed in, so tests
//! can supply their own and never touch the real home directory.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// A snapshot of environment variables, and the directory Whetstone
/// started in.
#[derive(Clone, Default)]
pub struct Environment {
    vars: BTreeMap<OsString, OsString>,
    current_dir: PathBuf,
}

impl Environment {
    /// Captures the current process's environment and current directory.
    /// A current directory that cannot be read, because it was removed,
    /// counts as `.`.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            current_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ..std::env::vars_os().collect()
        }
    }

    /// The directory Whetstone started in.
    #[must_use]
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    /// The value of `name`, if it is set.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars.get(OsStr::new(name)).map(OsString::as_os_str)
    }

    /// The system temporary directory: `TMPDIR` if it is an absolute path,
    /// otherwise `/tmp`, as `std::env::temp_dir` finds it on Linux, but
    /// from this snapshot rather than the process's environment. An empty
    /// or relative `TMPDIR` counts as unset, so nothing is ever created
    /// relative to the current directory. On macOS, where launchd always
    /// sets `TMPDIR`, `std` would ask the system for the per-user directory
    /// when it is unset; that takes a C call this crate does not make.
    #[must_use]
    pub fn temp_dir(&self) -> PathBuf {
        self.absolute_path("TMPDIR")
            .map_or_else(|| PathBuf::from("/tmp"), Path::to_path_buf)
    }

    /// The value of `name` as an absolute path. Unset, empty and relative
    /// values count as unset, as the XDG Base Directory spec requires.
    fn absolute_path(&self, name: &str) -> Option<&Path> {
        self.get(name)
            .map(Path::new)
            .filter(|path| path.is_absolute())
    }
}

impl<K: Into<OsString>, V: Into<OsString>> FromIterator<(K, V)> for Environment {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            vars: iter
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
            current_dir: PathBuf::new(),
        }
    }
}

/// Lists variable names only: values may be secrets.
impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment")
            .field("vars", &self.vars.keys().collect::<Vec<_>>())
            .field("current_dir", &self.current_dir)
            .finish()
    }
}

/// Where Whetstone keeps its files, following the XDG Base Directory layout
/// on Linux and macOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// The global config directory: `$XDG_CONFIG_HOME/whetstone`, by default
    /// `~/.config/whetstone`.
    pub config: PathBuf,
    /// The state directory, which holds the log file:
    /// `$XDG_STATE_HOME/whetstone`, by default `~/.local/state/whetstone`.
    pub state: PathBuf,
    /// The cache directory, which holds connected-code graphs:
    /// `$XDG_CACHE_HOME/whetstone`, by default `~/.cache/whetstone`.
    pub cache: PathBuf,
}

/// Neither an XDG variable nor `HOME` says where a directory is.
#[derive(Debug, thiserror::Error)]
#[error(
    "cannot find the {} directory: set HOME or {} to an absolute path",
    .dir.name,
    .dir.xdg_variable
)]
pub struct NoHome {
    dir: BaseDir,
}

impl Dirs {
    /// Resolves the directories from `environment`.
    pub fn resolve(environment: &Environment) -> Result<Self, NoHome> {
        Ok(Self {
            config: BaseDir::CONFIG.resolve(environment)?,
            state: BaseDir::STATE.resolve(environment)?,
            cache: BaseDir::CACHE.resolve(environment)?,
        })
    }
}

/// One XDG base directory: where its variable points, or its place under
/// `HOME`.
#[derive(Debug, Clone, Copy)]
struct BaseDir {
    name: &'static str,
    xdg_variable: &'static str,
    under_home: &'static str,
}

impl BaseDir {
    const CONFIG: Self = Self {
        name: "config",
        xdg_variable: "XDG_CONFIG_HOME",
        under_home: ".config",
    };
    const STATE: Self = Self {
        name: "state",
        xdg_variable: "XDG_STATE_HOME",
        under_home: ".local/state",
    };
    const CACHE: Self = Self {
        name: "cache",
        xdg_variable: "XDG_CACHE_HOME",
        under_home: ".cache",
    };

    /// Whetstone's folder inside this base directory.
    fn resolve(self, environment: &Environment) -> Result<PathBuf, NoHome> {
        let base = environment
            .absolute_path(self.xdg_variable)
            .map(Path::to_path_buf)
            .or_else(|| {
                environment
                    .absolute_path("HOME")
                    .map(|home| home.join(self.under_home))
            })
            .ok_or(NoHome { dir: self })?;
        Ok(base.join("whetstone"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temporary_directory_is_tmpdir_when_it_is_an_absolute_path() {
        let environment: Environment = [("TMPDIR", "/var/folders/xy/T/")].into_iter().collect();

        assert_eq!(environment.temp_dir(), Path::new("/var/folders/xy/T/"));
    }

    #[test]
    fn the_temporary_directory_is_tmp_when_tmpdir_is_unset_empty_or_relative() {
        for vars in [vec![], vec![("TMPDIR", "")], vec![("TMPDIR", "scratch")]] {
            let environment: Environment = vars.into_iter().collect();

            assert_eq!(environment.temp_dir(), Path::new("/tmp"), "{environment:?}");
        }
    }
}
