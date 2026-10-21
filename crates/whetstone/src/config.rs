//! The global config: what Practice will use.
//!
//! It is `config.toml` in the global config directory, by default
//! `~/.config/whetstone/config.toml`. Every setting is optional:
//!
//! ```toml
//! provider = "openrouter"
//! colours = "auto"
//! connected_code_limit_kb = 100
//! questions = 4
//!
//! [jev]
//! model = "typesafe/jev-1.14"
//!
//! [generator]
//! model = "anthropic/claude-sonnet-5.5"
//!
//! [cut_offs]
//! relevance_gate = { upper = 0.80, lower = 0.20 }
//! question_check = { upper = 0.80, lower = 0.20 }
//! giveaway_check = { upper = 0.50, lower = 0.20 }
//! option_rating = { upper = 0.80, lower = 0.20 }
//! rationale_and_hint_checks = { upper = 0.80, lower = 0.20 }
//! reasoning_criterion = { upper = 0.80, lower = 0.20 }
//! explanation_check = { upper = 0.80, lower = 0.20 }
//! ```
//!
//! The values above are the defaults, used for anything the file leaves out
//! and for everything when there is no file. The cut-offs are the
//! assessment policy's, the current policy's by default: a job's own makes its
//! outcomes not comparable with the policy's, and every outcome records the
//! cut-offs that banded it. Only this config sets them, never a
//! repository's. The API key never lives in the
//! file: it is read from `OPENROUTER_API_KEY`, once, as the config loads,
//! and Practice asks Jev and the generator with it ([`Settings::openrouter`]).

use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

use serde::de::{DeserializeSeed, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use whetstone_ai::{Models, OPENROUTER_BASE_URL, OpenRouter, OpenRouterAi, SecretString, Timeouts};
use whetstone_core::{
    AssessmentPolicy, ConnectedCodeLimit, ConnectedCodeLimitError, ConnectedCodeLimitKb, CutOffs,
    InvalidCutOffs, Job, ModelId, QuestionCount, QuestionCountError, RepositoryConfig,
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
/// whether `OPENROUTER_API_KEY` is set. The key is kept only as a
/// `SecretString`, whose value is never shown (rule 41).
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
    /// How many questions a Practice asks for.
    pub(crate) questions: Setting<QuestionCount>,
    /// The assessment policy Practice applies: the current policy, with the
    /// cut-offs `[cut_offs]` sets for any job.
    pub(crate) assessment_policy: AssessmentPolicy,
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

/// Whether the environment sets `OPENROUTER_API_KEY` to a key, text that
/// isn't blank, and the key, read once into a `SecretString` (rule 41). Its
/// `Debug` output redacts the key (rule 52).
#[derive(Debug, Clone)]
pub(crate) enum KeyStatus {
    Set(SecretString),
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
    #[serde(default, deserialize_with = "questions")]
    questions: Option<QuestionCount>,
    jev: Option<Role>,
    generator: Option<Role>,
    cut_offs: Option<CutOffsTable>,
}

/// The `[cut_offs]` table as written: each job it has an entry for, with
/// that entry's cut-offs, in the order written.
#[derive(Debug, Default)]
struct CutOffsTable(Vec<(Job, CutOffs)>);

/// A job's key in `[cut_offs]`, which reads that job's entry.
#[derive(Debug, Clone, Copy)]
struct CutOffsKey(Job);

/// The settings of one AI role.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Role {
    model: Option<String>,
}

impl Settings {
    /// Reads `config.toml` in `config_dir`, falling back to the defaults,
    /// and the key `environment` sets in `OPENROUTER_API_KEY`, if it sets
    /// one.
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
            questions: Setting::or_default(contents.questions, QuestionCount::DEFAULT),
            assessment_policy: AssessmentPolicy::with_cut_offs(
                contents.cut_offs.unwrap_or_default().0,
            ),
            file,
            openrouter_key: openrouter_key(environment).map_or(KeyStatus::NotSet, |key| {
                KeyStatus::Set(SecretString::from(key.to_owned()))
            }),
        })
    }

    /// Jev and the generator on `OpenRouter`, asked for the models this
    /// config names with the key `OPENROUTER_API_KEY` set, which only the auth
    /// header exposes (rule 41), within Whetstone's time limits: 10 s to
    /// connect, 30 s for a Jev call and 120 s for a generator call (rule 22).
    /// Without a key there is none, and Practice can't start. Nor can it
    /// when the HTTP client can't be built, which only a broken TLS setup
    /// causes: that is logged.
    #[must_use]
    pub fn openrouter(&self) -> Option<OpenRouterAi> {
        self.openrouter_at(OPENROUTER_BASE_URL, Timeouts::default())
    }

    /// The same as [`Settings::openrouter`], but at `base_url` and within
    /// `timeouts`, as a test gives its local server and shorter limits.
    #[must_use]
    pub fn openrouter_at(&self, base_url: &str, timeouts: Timeouts) -> Option<OpenRouterAi> {
        let KeyStatus::Set(key) = &self.openrouter_key else {
            return None;
        };
        match OpenRouter::with_settings(Some(key.clone()), base_url, timeouts) {
            Ok(connection) => Some(OpenRouterAi::new(connection, self.models())),
            Err(failure) => {
                tracing::warn!(%failure, "Practice can't reach OpenRouter");
                None
            }
        }
    }

    /// The models Practice asks: Jev's and the generator's, as this config
    /// names them.
    pub(crate) fn models(&self) -> Models {
        Models {
            jev: ModelId::new(self.jev_model.value.clone()),
            generator: ModelId::new(self.generator_model.value.clone()),
        }
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

/// The key `environment` sets in `OPENROUTER_API_KEY`, if it sets one:
/// text that isn't blank.
fn openrouter_key(environment: &Environment) -> Option<&str> {
    environment
        .get(OPENROUTER_KEY_VARIABLE)?
        .to_str()
        .filter(|key| !key.trim().is_empty())
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

/// Reads `questions`, which must be a whole number from 3 to 5.
fn questions<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<QuestionCount>, D::Error> {
    toml::Value::deserialize(deserializer)?
        .as_integer()
        .ok_or(QuestionCountError)
        .and_then(QuestionCount::try_from)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// `job`'s key in `[cut_offs]`.
pub(crate) fn cut_offs_key(job: Job) -> &'static str {
    match job {
        Job::RelevanceGate => "relevance_gate",
        Job::QuestionCheck => "question_check",
        Job::GiveawayCheck => "giveaway_check",
        Job::OptionRating => "option_rating",
        Job::RationaleAndHintChecks => "rationale_and_hint_checks",
        Job::ReasoningCriterion => "reasoning_criterion",
        Job::ExplanationCheck => "explanation_check",
    }
}

impl<'de> Deserialize<'de> for CutOffsTable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(CutOffsEntries)
    }
}

/// Reads the entries of `[cut_offs]`.
#[derive(Debug)]
struct CutOffsEntries;

impl<'de> Visitor<'de> for CutOffsEntries {
    type Value = CutOffsTable;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the `[cut_offs]` table, with an entry for each job it sets")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<CutOffsTable, A::Error> {
        let mut table = Vec::new();
        while let Some(key) = entries.next_key::<CutOffsKey>()? {
            table.push((key.0, entries.next_value_seed(key)?));
        }
        Ok(CutOffsTable(table))
    }
}

impl<'de> Deserialize<'de> for CutOffsKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Job::ALL
            .into_iter()
            .find(|&job| cut_offs_key(job) == key)
            .map(CutOffsKey)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "`cut_offs.{key}` is not a job: use {}",
                    Job::ALL.map(cut_offs_key).join(", ")
                ))
            })
    }
}

/// Reads the job's entry: a table of an `upper` and a `lower` cut-off, each
/// a number from 0 to 1, with `lower` below `upper`.
impl<'de> DeserializeSeed<'de> for CutOffsKey {
    type Value = CutOffs;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<CutOffs, D::Error> {
        let key = cut_offs_key(self.0);
        let entry = toml::Value::deserialize(deserializer)?;
        let both =
            || serde::de::Error::custom(format!("`cut_offs.{key}` needs both `upper` and `lower`"));
        let entry = entry.as_table().ok_or_else(both)?;
        if let Some(other) = entry
            .keys()
            .find(|name| !["upper", "lower"].contains(&name.as_str()))
        {
            return Err(serde::de::Error::custom(format!(
                "`cut_offs.{key}` takes only `upper` and `lower`, not `{other}`"
            )));
        }
        let (Some(upper), Some(lower)) = (entry.get("upper"), entry.get("lower")) else {
            return Err(both());
        };
        let invalid =
            |error: InvalidCutOffs| serde::de::Error::custom(format!("`cut_offs.{key}`: {error}"));
        let upper = number(upper)
            .ok_or(InvalidCutOffs::Upper)
            .map_err(invalid)?;
        let lower = number(lower)
            .ok_or(InvalidCutOffs::Lower)
            .map_err(invalid)?;
        CutOffs::new(upper, lower).map_err(invalid)
    }
}

/// `value` as a number, if it is one: a float, or a whole number.
fn number(value: &toml::Value) -> Option<f64> {
    value.as_float().or_else(|| {
        value
            .as_integer()
            .and_then(|whole| i32::try_from(whole).ok())
            .map(f64::from)
    })
}

/// `path` as Settings shows it: under the home directory it starts `~/`.
pub(crate) fn shown_path(path: &Path, environment: &Environment) -> String {
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
