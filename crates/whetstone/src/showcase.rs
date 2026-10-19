//! The showcase project as the binary carries it. `build.rs` replays its
//! patch series at build time and generates the code included here, so it
//! needs no git and no files at runtime and works from any install.

use whetstone_core::{Area, Change, ShowcaseProject, TouchedFile};

// `declared_areas()`, `changes()` and the file `CONTENTS` they share.
include!(concat!(env!("OUT_DIR"), "/showcase.rs"));

/// The showcase project that ships with Whetstone: the areas its config
/// declares and its curated changes.
#[must_use]
pub fn embedded_showcase() -> ShowcaseProject {
    ShowcaseProject::new(declared_areas(), changes())
}
