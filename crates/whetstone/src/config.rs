//! The global config: what Practice will use.
//!
//! It is `config.toml` in the global config directory, by default
//! `~/.config/whetstone/config.toml`. Every setting is optional:
//!
//! ```toml
//! provider = "openrouter"
//! colours = "auto"
//! connected_code_limit_kb = 100
//!
//! [jev]
//! model = "typesafe/jev-1.14"
//!
//! [generator]
//! model = "anthropic/claude-sonnet-5.5"
//! ```
//!
//! The values above are the defaults, used for anything the file leaves out
//! and for everything when there is no file. The API key never lives in the
//! file: it is read from `OPENROUTER_API_KEY`.

use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

use serde::{Deserialize, Deserializer};
use whetstone_core::{
    ConnectedCodeLimit, ConnectedCodeLimitError, ConnectedCodeLimitKb, RepositoryConfig,
};

use crate::environment::Environment;
use crate::theme::{ColorSupport, Colours, NoColorStatus};

/// The model Jev runs on when the config names none.
pub(crate) const DEFAULT_JEV_MODEL: &str = "typesafe/jev-1.14";
/// The generator's model when the config names none.
pub(crate) const DEFAULT_GENERATOR_MODEL: &str = "anthropic/claude-sonnet-5.5";
/// The environment variable the API key is read from.
pub(crate) const OPENROUTER_KEY_VARIABLE: &str = "OPENROUTER_API_KEY";

/// What Practice will use, as Settings shows it: the global config, and
/// whether `OPENROUTER_API_KEY` is set. The key's value is never kept.
#[derive(Debug, Clone)]
pub struct Settings {
    pub(crate) provider: Setting<Provider>,
    pub(crate) jev_model: Setting<String>,
    pub(crate) generator_model: Setting<String>,
    pub(crate) colours: Setting<Colours>,
    /// The level auto detects from the terminal, whatever `colours` says.
    pub(crate) detected_color_support: ColorSupport,
    /// Whether `NO_COLOR` turns colour off, whatever `colours` says.
    pub(crate) no_color: NoColorStatus,
    pub(crate) connected_code_limit: Setting<ConnectedCodeLimitKb>,
    pub(crate) file: ConfigFile,
    pub(crate) openrouter_key: KeyStatus,
}

/// A setting's value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setting<T> {
    pub(crate) value: T,
    pub(crate) origin: Origin,
}

/// Where a setting's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The config file left it out, or there is no config file.
    Default,
    /// The config file sets it.
    File,
}

/// Which service Practice will send its requests to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Provider {
    OpenRouter,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenRouter => "OpenRouter",
        })
    }
}

/// Whether the global config file exists, and where it is or would be, as
/// Settings shows the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigFile {
    Found { shown: String },
    Missing { shown: String },
}

/// Whether the environment sets `OPENROUTER_API_KEY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyStatus {
    Set,
    NotSet,
}

/// The global config file exists but cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file cannot be read.
    #[error("cannot read the config file {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The file is not a valid Whetstone config.
    #[error("the config file {} is not valid", path.display())]
    Invalid {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// The config file as written. Every setting is optional, and an unknown
/// one is an error rather than silently ignored, so a typo shows up.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    provider: Option<Provider>,
    colours: Option<Colours>,
    #[serde(default, deserialize_with = "connected_code_limit_kb")]
    connected_code_limit_kb: Option<ConnectedCodeLimitKb>,
    jev: Option<Role>,
    generator: Option<Role>,
}

/// The settings of one AI role.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Role {
    model: Option<String>,
}

impl Settings {
    /// Reads `config.toml` in `config_dir`, falling back to the defaults,
    /// and whether `environment` sets `OPENROUTER_API_KEY`.
    pub fn load(config_dir: &Path, environment: &Environment) -> Result<Self, ConfigError> {
        let path = config_dir.join("config.toml");
        let shown = shown_path(&path, environment);
        let (file, contents) = match fs::read_to_string(&path) {
            Ok(text) => {
                let contents =
                    toml::from_str::<File>(&text).map_err(|source| ConfigError::Invalid {
                        path: path.clone(),
                        source,
                    })?;
                tracing::info!(path = %path.display(), "read the global config");
                (ConfigFile::Found { shown }, contents)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::info!(path = %path.display(), "no global config, so the defaults apply");
                (ConfigFile::Missing { shown }, File::default())
            }
            Err(source) => return Err(ConfigError::Read { path, source }),
        };
        let model = |role: Option<Role>, default: &str| {
            Setting::or_default(role.and_then(|role| role.model), default.to_owned())
        };
        Ok(Self {
            provider: Setting::or_default(contents.provider, Provider::OpenRouter),
            jev_model: model(contents.jev, DEFAULT_JEV_MODEL),
            generator_model: model(contents.generator, DEFAULT_GENERATOR_MODEL),
            colours: Setting::or_default(contents.colours, Colours::Auto),
            detected_color_support: ColorSupport::detect(environment),
            no_color: NoColorStatus::read(environment),
            connected_code_limit: Setting::or_default(
                contents.connected_code_limit_kb,
                ConnectedCodeLimitKb::DEFAULT,
            ),
            file,
            openrouter_key: if environment
                .get(OPENROUTER_KEY_VARIABLE)
                .is_some_and(|value| !value.is_empty())
            {
                KeyStatus::Set
            } else {
                KeyStatus::NotSet
            },
        })
    }

    /// The colour level to draw with: none under a non-empty `NO_COLOR`,
    /// otherwise the `colours` setting, otherwise what the terminal reports.
    #[must_use]
    pub fn color_support(&self) -> ColorSupport {
        ColorSupport::choose(
            self.no_color,
            self.colours.value,
            self.detected_color_support,
        )
    }

    /// The connected code limit that applies to a Practice on a repository
    /// whose config is `repository`: this config's, or the default when it
    /// sets none, lowered by the repository's when that is smaller.
    #[must_use]
    pub fn connected_code_limit(&self, repository: &RepositoryConfig) -> ConnectedCodeLimit {
        let learner = match self.connected_code_limit.origin {
            Origin::File => Some(self.connected_code_limit.value),
            Origin::Default => None,
        };
        ConnectedCodeLimit::effective(learner, repository.connected_code_limit_kb())
    }
}

impl<T> Setting<T> {
    /// The file's value if it sets one, or else `default`.
    fn or_default(from_file: Option<T>, default: T) -> Self {
        match from_file {
            Some(value) => Self {
                value,
                origin: Origin::File,
            },
            None => Self {
                value: default,
                origin: Origin::Default,
            },
        }
    }
}

/// Reads `connected_code_limit_kb`, which must be a whole number from 0 to
/// 400.
fn connected_code_limit_kb<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ConnectedCodeLimitKb>, D::Error> {
    toml::Value::deserialize(deserializer)?
        .as_integer()
        .ok_or(ConnectedCodeLimitError)
        .and_then(ConnectedCodeLimitKb::try_from)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// `path` as Settings shows it: under the home directory it starts `~/`.
fn shown_path(path: &Path, environment: &Environment) -> String {
    environment
        .get("HOME")
        .map(Path::new)
        .filter(|home| home.is_absolute())
        .and_then(|home| path.strip_prefix(home).ok())
        .map_or_else(
            || path.display().to_string(),
            |under_home| format!("~/{}", under_home.display()),
        )
}
