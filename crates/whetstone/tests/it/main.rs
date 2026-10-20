//! Integration tests, built as one crate. `panic_restore.rs` runs on its own
//! because it replaces the process-wide panic hook.

mod app;
mod areas;
mod areas_init;
/// The build script's checks on areas, which it shares with these tests.
#[path = "../../build/areas.rs"]
mod build_areas;
mod cli;
mod colors;
mod commit_viewer;
mod commits;
mod config;
mod dirs;
mod navigation;
mod outbound_review;
mod overlays;
mod paste;
mod picker;
mod pty;
mod pty_colours;
mod pty_dumb;
mod pty_terminal;
mod repo;
mod repository;
mod resize;
mod session;
mod settings;
mod showcase;
mod support;
mod terminal_guard;
mod theme;
mod viewer;
mod wide_characters;
