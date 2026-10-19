//! A repository's Whetstone config: `.whetstone/config.toml` at its Git
//! root. The file is optional, and its only key is `exclude`, the paths
//! Practice leaves out, as gitignore-style patterns matched against
//! repository-relative paths:
//!
//! ```toml
//! exclude = ["fixtures/", "*.snap"]
//! ```
//!
//! A `!` negation pattern is invalid, and so is every other key, including
//! the old `areas` array and any model, endpoint or key setting, so a cloned
//! repository's config can only add exclusions.

use crate::exclusion::{ExcludePatternError, ExcludePatterns};

/// A repository's Whetstone config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryConfig {
    exclude: ExcludePatterns,
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
    /// The file has a key other than `exclude`.
    #[error("the key `{0}` is not allowed, only `exclude`")]
    UnknownKey(String),
    /// `exclude` is not a list of strings.
    #[error("`exclude` must be a list of patterns")]
    ExcludeNotAList,
    /// A pattern in `exclude` is invalid, such as a `!` negation.
    #[error(transparent)]
    Pattern(#[from] ExcludePatternError),
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
            let start = error.span().map_or(0, |span| span.start);
            RepositoryConfigError::Toml {
                line: 1 + text
                    .bytes()
                    .take(start)
                    .filter(|&byte| byte == b'\n')
                    .count(),
                message: error.message().trim().to_owned(),
            }
        })?;
        if let Some(key) = table.keys().find(|key| key.as_str() != "exclude") {
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
        Ok(Self { exclude })
    }

    /// The patterns of the paths Practice leaves out, in the order the file
    /// lists them.
    #[must_use]
    pub fn exclude(&self) -> &ExcludePatterns {
        &self.exclude
    }
}
