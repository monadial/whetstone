//! Integration tests, built as one crate. `panic_restore.rs` runs on its own
//! because it replaces the process-wide panic hook.

mod app;
mod areas;
mod areas_init;
mod cli;
mod colors;
mod commit_viewer;
mod commits;
mod config;
mod dirs;
mod navigation;
mod overlays;
mod picker;
mod pty;
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
