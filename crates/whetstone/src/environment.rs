//! The environment Whetstone runs in, captured once and passed in, so tests
//! can supply their own and never touch the real home directory.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// A snapshot of environment variables.
#[derive(Clone, Default)]
pub struct Environment {
    vars: BTreeMap<OsString, OsString>,
}

impl Environment {
    /// Captures the current process's environment.
    #[must_use]
    pub fn from_process() -> Self {
        std::env::vars_os().collect()
    }

    /// The value of `name`, if it is set.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars.get(OsStr::new(name)).map(OsString::as_os_str)
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
        }
    }
}

/// Lists variable names only: values may be secrets.
impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.vars.keys()).finish()
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
}

/// Neither an XDG variable nor `HOME` says where a directory is.
#[derive(Debug, thiserror::Error)]
#[error("cannot find the {kind} directory: set HOME or {xdg_variable} to an absolute path")]
pub struct NoHome {
    kind: &'static str,
    xdg_variable: &'static str,
}

impl Dirs {
    /// Resolves the directories from `environment`.
    pub fn resolve(environment: &Environment) -> Result<Self, NoHome> {
        Ok(Self {
            config: user_dir(environment, "config", "XDG_CONFIG_HOME", ".config")?,
            state: user_dir(environment, "state", "XDG_STATE_HOME", ".local/state")?,
        })
    }
}

fn user_dir(
    environment: &Environment,
    kind: &'static str,
    xdg_variable: &'static str,
    under_home: &str,
) -> Result<PathBuf, NoHome> {
    let base = environment
        .absolute_path(xdg_variable)
        .map(Path::to_path_buf)
        .or_else(|| {
            environment
                .absolute_path("HOME")
                .map(|home| home.join(under_home))
        })
        .ok_or(NoHome { kind, xdg_variable })?;
    Ok(base.join("whetstone"))
}
