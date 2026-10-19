//! The CLI output path: the only place that prints text to stdout or stderr.
//! Nothing here runs while the TUI owns the terminal.

use std::path::PathBuf;
use std::process::ExitCode;

/// Prints a clap outcome (help, version or a usage error) to the stream clap
/// chooses, and returns clap's exit code for it.
pub(crate) fn clap_outcome(outcome: &clap::Error) -> ExitCode {
    // Nothing useful remains to report if the stream itself is gone.
    drop(outcome.print());
    ExitCode::from(u8::try_from(outcome.exit_code()).unwrap_or(FAILURE))
}

/// Prints each of `paths`, one to a line, to stdout: the files a command
/// wrote.
pub(crate) fn written(paths: &[PathBuf]) {
    #[expect(
        clippy::print_stdout,
        reason = "the CLI output path reports what a command wrote, without a TUI"
    )]
    for path in paths {
        println!("{}", path.display());
    }
}

/// Prints an error that ended the run to stderr and returns the failure
/// exit code.
pub(crate) fn failure(error: &anyhow::Error) -> ExitCode {
    #[expect(
        clippy::print_stderr,
        reason = "the CLI output path reports errors after the TUI has released the terminal"
    )]
    {
        eprintln!("error: {error:#}");
    }
    ExitCode::from(FAILURE)
}

/// The exit code for a run that failed after its arguments were accepted.
pub(crate) const FAILURE: u8 = 1;
