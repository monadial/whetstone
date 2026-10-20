//! The connected code limit: how much connected code a Practice may add to
//! its evidence. The learner's config sets it with `connected_code_limit_kb`,
//! by default 100 KB, and a repository's config can lower it, never raise it.

use std::fmt;

/// A connected code limit as a config sets it with `connected_code_limit_kb`:
/// a whole number of kilobytes from 0 to 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectedCodeLimitKb(u16);

/// A `connected_code_limit_kb` value is not a whole number from 0 to 400.
/// A config reader returns it for a value that is not an integer, too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("`connected_code_limit_kb` must be a whole number from 0 to 400")]
pub struct ConnectedCodeLimitError;

impl ConnectedCodeLimitKb {
    /// The limit when no config sets one.
    pub const DEFAULT: Self = Self(100);
    /// The highest limit.
    const MAX: u16 = 400;

    /// The limit in bytes. A kilobyte is 1,000 bytes, as in
    /// [`Evidence::SIZE_LIMIT`](crate::Evidence::SIZE_LIMIT).
    #[must_use]
    pub fn bytes(self) -> usize {
        usize::from(self.0) * 1000
    }
}

impl fmt::Display for ConnectedCodeLimitKb {
    /// Shows the limit as Settings does: `100 KB`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} KB", self.0)
    }
}

impl TryFrom<i64> for ConnectedCodeLimitKb {
    type Error = ConnectedCodeLimitError;

    fn try_from(kb: i64) -> Result<Self, Self::Error> {
        u16::try_from(kb)
            .ok()
            .filter(|&kb| kb <= Self::MAX)
            .map(Self)
            .ok_or(ConnectedCodeLimitError)
    }
}

/// The connected code limit that applies to a Practice, and where it comes
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedCodeLimit {
    kb: ConnectedCodeLimitKb,
    source: ConnectedCodeLimitSource,
}

/// Where the connected code limit that applies comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectedCodeLimitSource {
    /// Neither config sets it, or only the repository's, to as much or more.
    Default,
    /// The learner's config sets it, and the repository's sets none or one as
    /// large or larger.
    Learner,
    /// The repository's config sets a smaller limit than the learner's or
    /// the default.
    LoweredByRepository,
}

impl ConnectedCodeLimit {
    /// The limit that applies when the learner's config sets `learner` and
    /// the repository's sets `repository`: the smaller of the two, with the
    /// default standing in for the learner's when theirs sets none. A
    /// repository's limit that is as large or larger has no effect.
    ///
    /// ```
    /// use whetstone_core::{ConnectedCodeLimit, ConnectedCodeLimitKb, ConnectedCodeLimitSource};
    ///
    /// let kb = |kb| ConnectedCodeLimitKb::try_from(kb).expect("from 0 to 400");
    ///
    /// let lowered = ConnectedCodeLimit::effective(None, Some(kb(40)));
    /// assert_eq!(lowered.bytes(), 40_000);
    /// assert_eq!(lowered.source(), ConnectedCodeLimitSource::LoweredByRepository);
    ///
    /// let not_raised = ConnectedCodeLimit::effective(Some(kb(40)), Some(kb(250)));
    /// assert_eq!(not_raised.kb(), kb(40));
    /// assert_eq!(not_raised.source(), ConnectedCodeLimitSource::Learner);
    /// ```
    #[must_use]
    pub fn effective(
        learner: Option<ConnectedCodeLimitKb>,
        repository: Option<ConnectedCodeLimitKb>,
    ) -> Self {
        let (kb, source) = match learner {
            Some(kb) => (kb, ConnectedCodeLimitSource::Learner),
            None => (
                ConnectedCodeLimitKb::DEFAULT,
                ConnectedCodeLimitSource::Default,
            ),
        };
        match repository {
            Some(lower) if lower < kb => Self {
                kb: lower,
                source: ConnectedCodeLimitSource::LoweredByRepository,
            },
            _ => Self { kb, source },
        }
    }

    /// The limit in kilobytes.
    #[must_use]
    pub fn kb(self) -> ConnectedCodeLimitKb {
        self.kb
    }

    /// The limit in bytes.
    #[must_use]
    pub fn bytes(self) -> usize {
        self.kb.bytes()
    }

    /// Where the limit comes from.
    #[must_use]
    pub fn source(self) -> ConnectedCodeLimitSource {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb(kb: i64) -> ConnectedCodeLimitKb {
        ConnectedCodeLimitKb::try_from(kb).expect("the limit is in range")
    }

    #[test]
    fn a_limit_is_a_whole_number_of_kb_from_0_to_400() {
        for (value, bytes) in [(0, 0), (1, 1000), (400, 400_000)] {
            assert_eq!(kb(value).bytes(), bytes, "{value}");
        }
        for value in [-1, 401, i64::from(u16::MAX) + 1, i64::MIN, i64::MAX] {
            assert_eq!(
                ConnectedCodeLimitKb::try_from(value),
                Err(ConnectedCodeLimitError),
                "{value}"
            );
        }
    }

    #[test]
    fn a_limit_shows_as_kb() {
        assert_eq!(kb(0).to_string(), "0 KB");
        assert_eq!(kb(100).to_string(), "100 KB");
    }

    #[test]
    fn the_learners_limit_applies_when_the_repository_sets_none() {
        let limit = ConnectedCodeLimit::effective(Some(kb(250)), None);

        assert_eq!(limit.kb(), kb(250));
        assert_eq!(limit.bytes(), 250_000);
        assert_eq!(limit.source(), ConnectedCodeLimitSource::Learner);
    }

    #[test]
    fn without_either_setting_the_default_of_100_kb_applies() {
        let limit = ConnectedCodeLimit::effective(None, None);

        assert_eq!(limit.kb(), kb(100));
        assert_eq!(limit.bytes(), 100_000);
        assert_eq!(limit.source(), ConnectedCodeLimitSource::Default);
    }

    #[test]
    fn a_repository_lowers_the_learners_limit_or_the_default() {
        for learner in [Some(kb(250)), None] {
            let limit = ConnectedCodeLimit::effective(learner, Some(kb(40)));

            assert_eq!(limit.kb(), kb(40), "{learner:?}");
            assert_eq!(limit.bytes(), 40_000, "{learner:?}");
            assert_eq!(
                limit.source(),
                ConnectedCodeLimitSource::LoweredByRepository,
                "{learner:?}"
            );
        }
    }

    #[test]
    fn a_repository_cannot_raise_the_limit() {
        for (learner, repository, expected, source) in [
            (Some(40), 250, 40, ConnectedCodeLimitSource::Learner),
            (Some(40), 40, 40, ConnectedCodeLimitSource::Learner),
            (None, 400, 100, ConnectedCodeLimitSource::Default),
            (None, 100, 100, ConnectedCodeLimitSource::Default),
        ] {
            let limit = ConnectedCodeLimit::effective(learner.map(kb), Some(kb(repository)));

            assert_eq!(limit.kb(), kb(expected), "{learner:?}, {repository}");
            assert_eq!(limit.source(), source, "{learner:?}, {repository}");
        }
    }

    #[test]
    fn a_limit_of_0_from_either_config_sends_no_connected_code() {
        for (learner, repository, source) in [
            (Some(0), None, ConnectedCodeLimitSource::Learner),
            (Some(0), Some(0), ConnectedCodeLimitSource::Learner),
            (Some(0), Some(40), ConnectedCodeLimitSource::Learner),
            (
                Some(250),
                Some(0),
                ConnectedCodeLimitSource::LoweredByRepository,
            ),
            (None, Some(0), ConnectedCodeLimitSource::LoweredByRepository),
        ] {
            let limit = ConnectedCodeLimit::effective(learner.map(kb), repository.map(kb));

            assert_eq!(limit.bytes(), 0, "{learner:?}, {repository:?}");
            assert_eq!(limit.source(), source, "{learner:?}, {repository:?}");
        }
    }
}
