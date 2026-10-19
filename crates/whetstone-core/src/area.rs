//! Areas: the lenses on a change that questions focus on.

use std::fmt;

/// A lens on a change that questions focus on, such as `domain` or
/// `system design`. A project declares its own areas; without any it has
/// [`Area::defaults`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Area(String);

impl Area {
    /// The area called `name`, exactly as a project declares it.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The areas of a project that declares none: domain, observability,
    /// scalability, infrastructure and system design.
    #[must_use]
    pub fn defaults() -> Vec<Self> {
        [
            "domain",
            "observability",
            "scalability",
            "infrastructure",
            "system design",
        ]
        .into_iter()
        .map(Self::new)
        .collect()
    }

    /// The area's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Area {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
