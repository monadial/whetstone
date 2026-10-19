//! The showcase project as the binary carries it. `build.rs` replays its
//! patch series at build time and generates the data included here, so it
//! needs no git and no files at runtime and works from any install.

use whetstone_core::{Area, Change, ShowcaseProject, TouchedFile};

/// One change, as `build.rs` embeds it.
#[derive(Debug)]
struct EmbeddedChange {
    message: &'static str,
    areas: &'static [&'static str],
    diff: &'static str,
    files: &'static [EmbeddedFile],
}

/// One touched file, as `build.rs` embeds it.
#[derive(Debug)]
struct EmbeddedFile {
    path: &'static str,
    before: Option<&'static str>,
    after: Option<&'static str>,
}

// `DECLARED_AREAS`, `CHANGES` and the `CONTENTS` they share.
include!(concat!(env!("OUT_DIR"), "/showcase.rs"));

/// The showcase project that ships with Whetstone: the areas its config
/// declares and its curated changes.
#[must_use]
pub fn embedded_showcase() -> ShowcaseProject {
    ShowcaseProject::new(
        DECLARED_AREAS.iter().copied().map(Area::new).collect(),
        CHANGES.iter().map(EmbeddedChange::to_change).collect(),
    )
}

impl EmbeddedChange {
    fn to_change(&self) -> Change {
        Change::new(
            self.message.to_owned(),
            self.areas.iter().copied().map(Area::new).collect(),
            self.diff.to_owned(),
            self.files
                .iter()
                .map(|file| {
                    TouchedFile::new(
                        file.path.to_owned(),
                        file.before.map(str::to_owned),
                        file.after.map(str::to_owned),
                    )
                })
                .collect(),
        )
    }
}
