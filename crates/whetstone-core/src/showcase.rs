//! The showcase project's catalogue: what a learner can practice on without
//! supplying their own source.

use crate::area::Area;
use crate::change::Change;

/// The small sample codebase that ships with Whetstone, as Practice sees it:
/// the areas it declares and its curated changes, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowcaseProject {
    areas: Vec<Area>,
    changes: Vec<Change>,
}

impl ShowcaseProject {
    /// The project with the areas its config declares and its curated
    /// `changes`. A project that declares no areas offers
    /// [`Area::defaults`]:
    ///
    /// ```
    /// use whetstone_core::{Area, ShowcaseProject};
    ///
    /// let project = ShowcaseProject::new(Vec::new(), Vec::new());
    ///
    /// assert_eq!(project.areas(), Area::defaults());
    /// ```
    #[must_use]
    pub fn new(declared_areas: Vec<Area>, changes: Vec<Change>) -> Self {
        Self {
            areas: Area::offered(declared_areas),
            changes,
        }
    }

    /// The areas a learner chooses from.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// The curated changes, oldest first.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
}
