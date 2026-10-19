//! The showcase project as the binary carries it. `build.rs` replays its
//! patch series at build time and generates the code included here, so it
//! needs no git and no files at runtime and works from any install.

use whetstone_core::{AreaId, Change, ShowcaseProject, TouchedFile};

// `changes()` and the file `CONTENTS` its changes share.
include!(concat!(env!("OUT_DIR"), "/showcase.rs"));

/// The showcase project that ships with Whetstone: its curated changes, and
/// the default areas, since it ships no area files.
#[must_use]
pub fn embedded_showcase() -> ShowcaseProject {
    ShowcaseProject::new(changes())
}

/// The area `id` that a change's `Areas:` trailer names.
fn area(id: &str) -> AreaId {
    id.parse()
        .expect("build.rs checks that every `Areas:` trailer names area IDs")
}
