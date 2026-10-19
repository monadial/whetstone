//! Command-line arguments.

use clap::Parser;

/// Whetstone helps learners examine their understanding of supplied work
/// through questions and written reasoning.
///
/// Run without arguments in a terminal to open the app.
#[derive(Debug, Parser)]
#[command(name = "whetstone", version)]
pub(crate) struct Cli {}
