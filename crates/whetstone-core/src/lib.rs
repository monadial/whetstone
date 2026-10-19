//! Domain types for Whetstone Practice.
//!
//! This crate holds the vocabulary from `GLOSSARY.md` (Practice, Showcase
//! project, Change, Area, ...) as plain data and pure logic. It performs no
//! I/O: no files, network, processes, environment or terminal. Its
//! `clippy.toml` disallows the common standard-library I/O entry points, so
//! lints catch most breaches; review covers the rest.

mod area;
mod change;
mod exclusion;
mod secret;
mod showcase;

pub use crate::area::{Area, AreaFileError, AreaId, AreaSource, InvalidAreaFile, InvalidAreaId};
pub use crate::change::{Change, TouchedFile};
pub use crate::exclusion::{
    DefaultExclusion, DefaultExclusionKind, ExcludePattern, ExcludePatternError,
    ExcludePatternProblem, ExcludePatterns,
};
pub use crate::secret::{
    SECRET_RULES, SECRET_RULES_VERSION, SecretFinding, SecretRule, find_secrets,
};
pub use crate::showcase::ShowcaseProject;
