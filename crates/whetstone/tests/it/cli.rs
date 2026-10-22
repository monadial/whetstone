//! Process seam: run the built `whetstone` binary without a terminal and
//! assert its stdout, stderr and exit code.

use std::process::Output;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

use crate::version::built_version;

/// Runs `whetstone` with `args` in a clean environment whose home is a fresh
/// temporary directory, with stdin and stdout piped rather than a terminal.
fn whetstone(args: &[&str]) -> (Output, TempDir) {
    let home = TempDir::new().expect("create a temporary home directory");
    let output = cargo_bin_cmd!("whetstone")
        .env_clear()
        .env("HOME", home.path())
        .args(args)
        .output()
        .expect("run the whetstone binary");
    (output, home)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn version_names_the_commit_it_was_built_from_on_stdout_and_exits_successfully() {
    let (output, _home) = whetstone(&["--version"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), format!("whetstone {}\n", built_version()));
    assert_eq!(stderr(&output), "");
}

#[test]
fn short_version_prints_the_version_alone() {
    let (output, _home) = whetstone(&["-V"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        concat!("whetstone ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn help_describes_the_command_on_stdout_and_exits_successfully() {
    let (output, _home) = whetstone(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    let help = stdout(&output);
    assert!(
        help.contains("examine their understanding of supplied work"),
        "help describes Whetstone:\n{help}"
    );
    assert!(
        help.contains("Usage: whetstone"),
        "help shows usage:\n{help}"
    );
    assert!(help.contains("--version"), "help lists --version:\n{help}");
    assert_eq!(stderr(&output), "");
}

#[test]
fn invalid_argument_prints_error_and_usage_hint_to_stderr_with_usage_exit_code() {
    let (output, _home) = whetstone(&["--no-such-flag"]);

    // clap's exit code for usage errors.
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    let error = stderr(&output);
    assert!(
        error.contains("error: unexpected argument '--no-such-flag'"),
        "stderr names the bad argument:\n{error}"
    );
    assert!(
        error.contains("Usage: whetstone"),
        "stderr shows usage:\n{error}"
    );
    assert!(
        error.contains("--help"),
        "stderr points to --help:\n{error}"
    );
}

#[test]
fn without_a_terminal_it_refuses_on_stderr_without_drawing() {
    let (output, home) = whetstone(&[]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "", "nothing is drawn");
    let error = stderr(&output);
    assert!(
        error.starts_with("error: whetstone needs an interactive terminal"),
        "stderr explains the refusal:\n{error}"
    );
    assert!(
        error.contains("stdin and stdout are not terminals"),
        "stderr says what is missing:\n{error}"
    );
    assert!(
        std::fs::read_dir(home.path())
            .expect("read the temporary home")
            .next()
            .is_none(),
        "a refusal writes nothing, not even a log"
    );
}
