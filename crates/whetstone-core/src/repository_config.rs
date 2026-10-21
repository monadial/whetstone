//! A repository's Whetstone config: `.whetstone/config.toml` at its Git
//! root. The file is optional, and so are its two keys:
//!
//! ```toml
//! exclude = ["fixtures/", "*.snap"]
//! connected_code_limit_kb = 40
//! ```
//!
//! `exclude` lists the paths Practice leaves out, as gitignore-style
//! patterns matched against repository-relative paths. A `!` negation
//! pattern is invalid. `connected_code_limit_kb` is a whole number from 0 to
//! 400, and only lowers the learner's connected code limit, never raises it.
//! Every other key is invalid, including the old `areas` array, any model,
//! endpoint or key setting and the assessment policy's `[cut_offs]`, so a
//! cloned repository's config can only add exclusions and lower the limit:
//! it can never loosen how its learners are assessed.

use crate::connected_code_limit::{ConnectedCodeLimitError, ConnectedCodeLimitKb};
use crate::exclusion::{ExcludePatternError, ExcludePatterns};
use crate::toml_error;

/// A repository's Whetstone config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryConfig {
    exclude: ExcludePatterns,
    connected_code_limit_kb: Option<ConnectedCodeLimitKb>,
}

/// Why a repository's config is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryConfigError {
    /// The file is not UTF-8 text.
    #[error("it is not UTF-8 text")]
    NotUtf8,
    /// The file is not valid TOML.
    #[error("line {line} is not valid TOML: {message}")]
    Toml {
        /// The line of the file where the problem is, counting from 1.
        line: usize,
        /// What the problem is.
        message: String,
    },
    /// The file has a key other than `exclude` and `connected_code_limit_kb`.
    #[error("the key `{0}` is not allowed, only `exclude` and `connected_code_limit_kb`")]
    UnknownKey(String),
    /// `exclude` is not a list of strings.
    #[error("`exclude` must be a list of patterns")]
    ExcludeNotAList,
    /// A pattern in `exclude` is invalid, such as a `!` negation.
    #[error(transparent)]
    Pattern(#[from] ExcludePatternError),
    /// `connected_code_limit_kb` is not a whole number from 0 to 400.
    #[error(transparent)]
    ConnectedCodeLimit(#[from] ConnectedCodeLimitError),
}

impl RepositoryConfig {
    /// Parses the config file with the contents `contents`.
    ///
    /// ```
    /// use whetstone_core::{RepositoryConfig, RepositoryConfigError};
    ///
    /// let config = RepositoryConfig::parse(b"exclude = [\"fixtures/\"]\n").expect("valid");
    /// let left_out = config.exclude().first_match("fixtures/cards.json");
    /// assert_eq!(left_out.map(|pattern| pattern.as_str()), Some("fixtures/"));
    ///
    /// let negation = RepositoryConfig::parse(b"exclude = [\"!keep.txt\"]\n");
    /// assert!(matches!(negation, Err(RepositoryConfigError::Pattern(_))));
    ///
    /// let old = RepositoryConfig::parse(b"areas = [\"Domain\"]\n");
    /// assert_eq!(old, Err(RepositoryConfigError::UnknownKey("areas".to_owned())));
    /// ```
    pub fn parse(contents: &[u8]) -> Result<Self, RepositoryConfigError> {
        let text = str::from_utf8(contents).map_err(|_| RepositoryConfigError::NotUtf8)?;
        let table: toml::Table = text.parse().map_err(|error: toml::de::Error| {
            let (line, message) = toml_error::line_and_message(text, &error);
            RepositoryConfigError::Toml { line, message }
        })?;
        if let Some(key) = table
            .keys()
            .find(|key| !["exclude", "connected_code_limit_kb"].contains(&key.as_str()))
        {
            return Err(RepositoryConfigError::UnknownKey(key.clone()));
        }
        let exclude = match table.get("exclude") {
            None => ExcludePatterns::default(),
            Some(value) => {
                let patterns: Vec<&str> = value
                    .as_array()
                    .and_then(|patterns| patterns.iter().map(toml::Value::as_str).collect())
                    .ok_or(RepositoryConfigError::ExcludeNotAList)?;
                ExcludePatterns::parse(patterns)?
            }
        };
        let connected_code_limit_kb = table
            .get("connected_code_limit_kb")
            .map(|value| {
                value
                    .as_integer()
                    .ok_or(ConnectedCodeLimitError)
                    .and_then(ConnectedCodeLimitKb::try_from)
            })
            .transpose()?;
        Ok(Self {
            exclude,
            connected_code_limit_kb,
        })
    }

    /// The patterns of the paths Practice leaves out, in the order the file
    /// lists them.
    #[must_use]
    pub fn exclude(&self) -> &ExcludePatterns {
        &self.exclude
    }

    /// The connected code limit the file sets, if any. It can only lower the
    /// learner's limit, never raise it (see
    /// [`ConnectedCodeLimit::effective`](crate::ConnectedCodeLimit::effective)).
    #[must_use]
    pub fn connected_code_limit_kb(&self) -> Option<ConnectedCodeLimitKb> {
        self.connected_code_limit_kb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_config_can_set_the_connected_code_limit_alongside_exclude() {
        for (contents, limit) in [
            ("", None),
            ("exclude = [\"fixtures/\"]\n", None),
            ("connected_code_limit_kb = 0\n", Some(0)),
            (
                "exclude = [\"fixtures/\"]\nconnected_code_limit_kb = 40\n",
                Some(40),
            ),
            ("connected_code_limit_kb = 400\n", Some(400)),
        ] {
            let config = RepositoryConfig::parse(contents.as_bytes()).expect("valid");

            assert_eq!(
                config.connected_code_limit_kb(),
                limit.map(|kb| ConnectedCodeLimitKb::try_from(kb).expect("in range")),
                "{contents:?}"
            );
        }
    }

    #[test]
    fn a_repository_config_can_never_set_the_assessment_policy_s_cut_offs() {
        for contents in [
            "[cut_offs]\nquestion_check = { upper = 0.55, lower = 0.20 }\n",
            "cut_offs = {}\n",
        ] {
            assert_eq!(
                RepositoryConfig::parse(contents.as_bytes()),
                Err(RepositoryConfigError::UnknownKey("cut_offs".to_owned())),
                "{contents:?}"
            );
        }
    }
}
