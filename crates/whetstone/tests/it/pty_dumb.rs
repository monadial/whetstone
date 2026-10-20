//! Pseudo-terminal tests for `TERM=dumb`: a terminal that says it cannot
//! move the cursor is refused before anything is written to it.
//!
//! As in `pty.rs`, a shell inside the pseudo-terminal records the terminal
//! modes (`stty -a`) before and after `whetstone` runs. It also prints a line
//! right before `whetstone` starts, so a test can read exactly what
//! `whetstone` wrote to the terminal.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

use rexpect::session::{PtySession, spawn_command};
use tempfile::TempDir;
use whetstone_core::Area;

use crate::repo::TestRepo;

const TIMEOUT_MS: u64 = 10_000;

/// What the shell prints right before `whetstone` starts.
const STARTS: &str = "whetstone starts\r\n";

/// Runs `whetstone` as `launch` says, between two records of the terminal
/// modes.
fn script(launch: &str) -> String {
    format!(
        r#"stty rows 24 cols 80
echo "modes before:"; stty -a; echo "modes end"
echo "whetstone starts"
{launch}
echo "exit code: $?"
echo "modes after:"; stty -a; echo "modes end"
"#
    )
}

/// Starts the script in a pseudo-terminal whose shell has
/// `TERM=xterm-256color`, so each launch sets its own `TERM`. `env` is the
/// whole environment besides `PATH` and `TERM`, so the real home directory is
/// never used, and the script starts in its `HOME`, outside any repository.
fn start(launch: &str, env: &[(&str, &Path)]) -> PtySession {
    let home = env
        .iter()
        .find_map(|(name, dir)| (*name == "HOME").then_some(*dir))
        .expect("every session has a temporary HOME");
    let mut command = Command::new("sh");
    command
        .current_dir(home)
        .args(["-c", &script(launch)])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").expect("PATH is set"))
        .env("TERM", "xterm-256color")
        .env("WHETSTONE", env!("CARGO_BIN_EXE_whetstone"))
        .envs(env.iter().copied());
    spawn_command(command, Some(TIMEOUT_MS)).expect("spawn whetstone in a pseudo-terminal")
}

/// Everything the shell printed, from start to end of file.
struct Transcript(String);

impl Transcript {
    /// Reads until the shell exits.
    fn read(session: &mut PtySession) -> Self {
        Self(session.exp_eof().expect("shell exits"))
    }

    /// Everything `whetstone` wrote to the terminal.
    fn written(&self) -> &str {
        let (_, after) = self
            .0
            .split_once(STARTS)
            .expect("the shell started whetstone");
        after
            .split_once("exit code: ")
            .map(|(written, _)| written)
            .expect("the shell reports whetstone's exit code")
    }

    fn section(&self, start: &str) -> &str {
        assert!(
            self.0.contains(start),
            "transcript has {start:?}:\n{:?}",
            self.0
        );
        let (_, after) = self.0.split_once(start).expect("section starts");
        after
            .split_once("modes end")
            .map(|(section, _)| section)
            .expect("section ends")
    }

    fn assert_modes_unchanged(&self) {
        let before = self.section("modes before:");
        let after = self.section("modes after:");
        assert!(
            before.split_whitespace().any(|flag| flag == "icanon"),
            "the terminal starts in canonical mode:\n{before}"
        );
        assert_eq!(before, after, "terminal modes are restored exactly");
    }

    fn assert_exit_code(&self, code: i32) {
        assert!(
            self.0.contains(&format!("exit code: {code}\r\n")),
            "whetstone exits with {code}:\n{:?}",
            self.0
        );
    }
}

#[test]
fn with_term_dumb_it_refuses_before_writing_anything_else() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(r#"TERM=dumb "$WHETSTONE""#, &[("HOME", home.path())]);

    let transcript = Transcript::read(&mut session);

    transcript.assert_exit_code(1);
    assert_eq!(
        transcript.written(),
        "error: whetstone needs a terminal that can move the cursor, but TERM=dumb says this one cannot\r\n",
        "the refusal is the only thing written: no escape sequence comes first"
    );
    transcript.assert_modes_unchanged();
}

#[test]
fn with_term_dumb_the_refusal_goes_to_stderr_and_nothing_to_the_terminal() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(
        r#"TERM=dumb "$WHETSTONE" 2> "$HOME/stderr""#,
        &[("HOME", home.path())],
    );

    let transcript = Transcript::read(&mut session);

    transcript.assert_exit_code(1);
    assert_eq!(transcript.written(), "", "nothing reaches the terminal");
    assert_eq!(
        std::fs::read_to_string(home.path().join("stderr")).expect("read redirected stderr"),
        "error: whetstone needs a terminal that can move the cursor, but TERM=dumb says this one cannot\n"
    );
    transcript.assert_modes_unchanged();
}

#[test]
fn with_term_unset_it_draws_and_quits() {
    assert_draws_and_quits(r#"(unset TERM; "$WHETSTONE")"#);
}

#[test]
fn with_term_empty_it_draws_and_quits() {
    assert_draws_and_quits(r#"TERM= "$WHETSTONE""#);
}

/// Waits for the home screen `launch` draws, quits with `q`, and checks the
/// terminal was left as it was.
fn assert_draws_and_quits(launch: &str) {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(launch, &[("HOME", home.path())]);

    // Ratatui moves the cursor between words, so wait for a single word: the
    // home screen's last action.
    let until_home = session.exp_string("Quit").expect("home screen is drawn");
    session.send("q").expect("send q");
    session.flush().expect("flush q");
    let rest = session.exp_eof().expect("shell exits");
    let transcript = Transcript(format!("{until_home}Quit{rest}"));

    transcript.assert_exit_code(0);
    transcript.assert_modes_unchanged();
}

#[test]
fn with_term_dumb_version_prints_the_version() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(
        r#"TERM=dumb "$WHETSTONE" --version"#,
        &[("HOME", home.path())],
    );

    let transcript = Transcript::read(&mut session);

    transcript.assert_exit_code(0);
    assert_eq!(
        transcript.written(),
        concat!("whetstone ", env!("CARGO_PKG_VERSION"), "\r\n")
    );
}

#[test]
fn with_term_dumb_help_prints_the_usage() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(r#"TERM=dumb "$WHETSTONE" --help"#, &[("HOME", home.path())]);

    let transcript = Transcript::read(&mut session);

    transcript.assert_exit_code(0);
    let help = transcript.written();
    assert!(
        help.contains("Usage: whetstone"),
        "help shows usage:\n{help}"
    );
}

#[test]
fn with_term_dumb_areas_init_writes_the_default_area_files() {
    let home = TempDir::new().expect("create a temporary home");
    let repo = TestRepo::new("payments");
    let mut session = start(
        r#"cd "$REPO" && TERM=dumb "$WHETSTONE" areas init"#,
        &[("HOME", home.path()), ("REPO", repo.root())],
    );

    let transcript = Transcript::read(&mut session);

    transcript.assert_exit_code(0);
    let printed: Vec<&str> = transcript.written().lines().collect();
    assert_eq!(
        printed.len(),
        Area::default_files().len(),
        "one line per area file written:\n{printed:?}"
    );
    for &(name, contents) in Area::default_files() {
        assert_eq!(
            std::fs::read(repo.path(".whetstone/areas").join(name)).expect("read an area file"),
            contents,
            "{name} is written"
        );
    }
}
