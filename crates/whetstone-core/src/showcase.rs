//! The showcase project's catalogue: what a learner can practice on without
//! supplying their own source.

use std::collections::BTreeMap;

use crate::area::Area;
use crate::change::{Change, FileContent};

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

    /// Every file in the project just after the change at `position` among
    /// [`changes`](Self::changes), by path: its tree at that change, rebuilt
    /// by replaying the changes up to it, from an empty tree. `None` if
    /// there is no change at `position`.
    ///
    /// ```
    /// use whetstone_core::{Change, ShowcaseProject, TouchedFile};
    ///
    /// let s = str::to_owned;
    /// let change = |file| Change::new(s("A change"), Vec::new(), String::new(), vec![file]);
    /// let project = ShowcaseProject::new(vec![
    ///     change(TouchedFile::created(s("a.rs"), s("a"))),
    ///     change(TouchedFile::created(s("b.rs"), s("b"))),
    ///     change(TouchedFile::deleted(s("a.rs"), s("a"))),
    ///     change(TouchedFile::renamed(s("b.rs"), s("c.rs"), s("b"), s("c"))),
    /// ]);
    /// let paths = |position| {
    ///     let files = project.files_at(position)?;
    ///     Some(files.into_keys().collect::<Vec<_>>())
    /// };
    ///
    /// assert_eq!(paths(0), Some(vec!["a.rs"]));
    /// assert_eq!(paths(1), Some(vec!["a.rs", "b.rs"]));
    /// assert_eq!(paths(2), Some(vec!["b.rs"]));
    /// assert_eq!(paths(3), Some(vec!["c.rs"]));
    /// assert_eq!(paths(4), None);
    /// ```
    #[must_use]
    pub fn files_at(&self, position: usize) -> Option<BTreeMap<&str, &FileContent>> {
        let replayed = self.changes.get(..=position)?;
        let mut files = BTreeMap::new();
        for file in replayed.iter().flat_map(Change::files) {
            if let Some(from) = file.renamed_from() {
                files.remove(from);
            }
            match file.after() {
                Some(after) => files.insert(file.path(), after),
                None => files.remove(file.path()),
            };
        }
        Some(files)
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
