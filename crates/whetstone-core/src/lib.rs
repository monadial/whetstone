//! Domain types for Whetstone Practice.
//!
//! This crate holds the vocabulary from `GLOSSARY.md` (Practice, Showcase
//! project, Change, Area, ...) as plain data and pure logic. It performs no
//! I/O: no files, network, processes, environment or terminal. Its
//! `clippy.toml` disallows the common standard-library I/O entry points, so
//! lints catch most breaches; review covers the rest.

mod area;
mod change;
mod showcase;

pub use crate::area::Area;
pub use crate::change::{Change, TouchedFile};
pub use crate::showcase::ShowcaseProject;
