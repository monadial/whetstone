//! Command-line arguments.

use clap::{Parser, Subcommand};

/// Whetstone helps learners examine their understanding of supplied work
/// through questions and written reasoning.
///
/// Run without arguments in a terminal to open the app.
// `-V` prints the version alone, and `--version` adds the commit the binary
// was built from and its date (ADR 0003), which `build.rs` sets.
#[derive(Debug, Parser)]
#[command(
    name = "whetstone",
    version,
    long_version = env!("WHETSTONE_LONG_VERSION")
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

/// What to do instead of opening the app.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Work with the area files of the repository Whetstone runs in.
    #[command(subcommand)]
    Areas(AreasCommand),
}

/// What to do with area files.
#[derive(Debug, Subcommand)]
pub(crate) enum AreasCommand {
    /// Copy Whetstone's five default area files into `.whetstone/areas/` at
    /// the repository's root, as a starting point. Refuses if area files
    /// are already there.
    Init,
}
