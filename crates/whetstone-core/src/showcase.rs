//! The showcase project's catalogue: what a learner can practice on without
//! supplying their own source.

use crate::area::Area;
use crate::change::Change;

/// The small sample codebase that ships with Whetstone, as Practice sees it:
/// the areas it offers and its curated changes, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowcaseProject {
    areas: Vec<Area>,
    changes: Vec<Change>,
}

impl ShowcaseProject {
    /// The project with its curated `changes`. It ships no area files, so
    /// it offers [`Area::defaults`]:
    ///
    /// ```
    /// use whetstone_core::{Area, ShowcaseProject};
    ///
    /// let project = ShowcaseProject::new(Vec::new());
    ///
    /// assert_eq!(project.areas(), Area::defaults());
    /// ```
    #[must_use]
    pub fn new(changes: Vec<Change>) -> Self {
        Self {
            areas: Area::defaults(),
            changes,
        }
    }

    /// The areas a learner chooses from, in file-name order.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// The curated changes, oldest first.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// The names of the areas `change` touches, in the order the change
    /// gives them. An area the project does not offer shows as its ID.
    #[must_use]
    pub fn area_names<'a>(&'a self, change: &'a Change) -> Vec<&'a str> {
        change
            .areas()
            .iter()
            .map(|id| {
                self.areas
                    .iter()
                    .find(|area| area.id() == id)
                    .map_or(id.as_str(), Area::name)
            })
            .collect()
    }
}
