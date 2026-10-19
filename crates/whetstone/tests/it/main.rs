//! Integration tests, built as one crate. `panic_restore.rs` runs on its own
//! because it replaces the process-wide panic hook.

mod app;
mod cli;
mod colors;
mod config;
mod dirs;
mod navigation;
mod overlays;
mod pty;
mod resize;
mod session;
mod settings;
mod support;
mod terminal_guard;
mod theme;
