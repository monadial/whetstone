//! Pseudo-terminal smoke test: run the real binary in a pseudo-terminal, quit
//! it, and check the terminal was left exactly as it was.
//!
//! A shell inside the pseudo-terminal records the terminal modes (`stty -a`)
//! before and after `whetstone` runs, so the comparison sees what the next
//! program in that terminal would see.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use rexpect::session::{PtySession, spawn_command};
use tempfile::TempDir;

const TIMEOUT_MS: u64 = 10_000;

/// Runs `whetstone` as `launch` says, between two records of the terminal
/// modes.
fn script(launch: &str) -> String {
    format!(
        r#"stty rows 24 cols 80
echo "modes before:"; stty -a; echo "modes end"
{launch}
echo "exit code: $?"
echo "modes after:"; stty -a; echo "modes end"
"#
    )
}

/// Runs `whetstone` in the foreground, attached to the terminal.
pub(super) const LAUNCH: &str = r#""$WHETSTONE""#;

/// Runs `whetstone` in the foreground with colour turned off.
const LAUNCH_WITHOUT_COLOR: &str = r#"NO_COLOR=1 "$WHETSTONE""#;

/// Runs `whetstone` in the background, still attached to the terminal, and
/// waits for it. Its process ID goes to `$HOME/pid` so the test can signal it.
pub(super) const LAUNCH_IN_BACKGROUND: &str = r#"exec 3<&0
"$WHETSTONE" <&3 3<&- &
echo $! > "$HOME/pid"
wait $!"#;

const ENTER_ALTERNATE_SCREEN: &str = "\x1b[?1049h";
const LEAVE_ALTERNATE_SCREEN: &str = "\x1b[?1049l";
const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";

/// Everything the shell printed, from start to end of file.
pub(super) struct Transcript(pub(super) String);

/// Starts the script in a pseudo-terminal. `env` is the whole environment
/// besides `PATH` and `TERM`, so the real home directory is never used, and
/// the script starts in its `HOME`, outside any repository, so the one the
/// tests run in is never opened.
pub(super) fn start(launch: &str, env: &[(&str, &Path)]) -> PtySession {
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

pub(super) fn quit_with_q(session: &mut PtySession) -> Transcript {
    quit_with(session, |session| {
        session.send("q").expect("send q");
        session.flush().expect("flush q");
    })
}

/// Waits for the home screen, sends `keys`, and reads until the shell exits.
pub(super) fn quit_with(
    session: &mut PtySession,
    send_keys: impl FnOnce(&mut PtySession),
) -> Transcript {
    // Ratatui moves the cursor between words, so wait for a single word: the
    // home screen's last action.
    let until_home = session.exp_string("Quit").expect("home screen is drawn");
    send_keys(session);
    let rest = session.exp_eof().expect("shell exits");
    Transcript(format!("{until_home}Quit{rest}"))
}

impl Transcript {
    pub(super) fn section(&self, start: &str) -> &str {
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

    pub(super) fn assert_terminal_restored(&self) {
        self.assert_modes_unchanged();

        let tui = self
            .0
            .split_once(ENTER_ALTERNATE_SCREEN)
            .map(|(_, tui)| tui)
            .expect("whetstone entered the alternate screen");
        assert!(tui.contains(HIDE_CURSOR), "whetstone hid the cursor");
        let after_hide = tui.rsplit_once(HIDE_CURSOR).map_or("", |(_, rest)| rest);
        assert!(
            after_hide.contains(SHOW_CURSOR),
            "the cursor is shown again"
        );
        assert!(
            tui.contains(LEAVE_ALTERNATE_SCREEN),
            "whetstone left the alternate screen"
        );
    }

    /// The parameters of every SGR sequence (`CSI … m`) whetstone drew in the
    /// alternate screen, in order.
    pub(super) fn drawn_styles(&self) -> Vec<&str> {
        let (_, tui) = self
            .0
            .split_once(ENTER_ALTERNATE_SCREEN)
            .expect("whetstone entered the alternate screen");
        let (tui, _) = tui
            .split_once(LEAVE_ALTERNATE_SCREEN)
            .expect("whetstone left the alternate screen");
        tui.split("\x1b[")
            .skip(1)
            .filter_map(|sequence| {
                let end = sequence.find(|c: char| ('@'..='~').contains(&c))?;
                let (parameters, rest) = sequence.split_at(end);
                rest.starts_with('m').then_some(parameters)
            })
            .collect()
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

    pub(super) fn assert_exit_code(&self, code: i32) {
        assert!(
            self.0.contains(&format!("exit code: {code}")),
            "whetstone exits with {code}:\n{:?}",
            self.0
        );
    }
}

#[test]
fn q_quits_and_restores_the_terminal() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH, &[("HOME", home.path())]);

    let transcript = quit_with_q(&mut session);

    transcript.assert_exit_code(0);
    transcript.assert_terminal_restored();
}

#[test]
fn ctrl_c_quits_and_restores_the_terminal() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH, &[("HOME", home.path())]);

    let transcript = quit_with(&mut session, |session| {
        session.send_control('c').expect("send Ctrl+C");
    });

    transcript.assert_exit_code(0);
    transcript.assert_terminal_restored();
}

#[test]
fn a_termination_signal_from_outside_quits_and_restores_the_terminal() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH_IN_BACKGROUND, &[("HOME", home.path())]);

    let transcript = quit_with(&mut session, |_| {
        let pid = read_pid(&home.path().join("pid")).expect("whetstone's process ID");
        let kill = Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .expect("run kill");
        assert!(kill.success(), "kill -TERM {pid}");
    });

    // The shell convention for a process ended by SIGTERM: 128 + 15.
    transcript.assert_exit_code(143);
    transcript.assert_terminal_restored();
}

/// Reads a process ID the shell writes right after starting the process.
pub(super) fn read_pid(path: &Path) -> Option<String> {
    for _ in 0..50 {
        if let Ok(pid) = std::fs::read_to_string(path)
            && !pid.trim().is_empty()
        {
            return Some(pid.trim().to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn logs_go_to_a_file_in_the_xdg_state_directory_and_never_to_the_screen() {
    let home = TempDir::new().expect("create a temporary home");
    let state = TempDir::new().expect("create a temporary state directory");
    let mut session = start(
        LAUNCH,
        &[("HOME", home.path()), ("XDG_STATE_HOME", state.path())],
    );

    let transcript = quit_with_q(&mut session);

    let log = std::fs::read_to_string(state.path().join("whetstone/whetstone.log"))
        .expect("the log file is in the XDG state directory");
    assert!(log.contains("whetstone started"), "log:\n{log}");
    assert!(log.contains("whetstone quit"), "log:\n{log}");
    assert!(
        !transcript.0.contains("whetstone started"),
        "nothing is logged to the screen"
    );
}

#[test]
fn without_xdg_variables_logs_go_under_the_home_directory() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH, &[("HOME", home.path())]);

    quit_with_q(&mut session);

    let log = std::fs::read_to_string(home.path().join(".local/state/whetstone/whetstone.log"))
        .expect("the log file is in the default state directory");
    assert!(log.contains("whetstone started"), "log:\n{log}");
}

#[test]
fn with_stdout_redirected_it_refuses_without_touching_the_terminal() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(r#""$WHETSTONE" > "$HOME/stdout""#, &[("HOME", home.path())]);

    let transcript = Transcript(session.exp_eof().expect("shell exits"));

    transcript.assert_exit_code(1);
    assert!(
        transcript.0.contains(
            "error: whetstone needs an interactive terminal, but stdout is not a terminal"
        ),
        "the refusal is on stderr, in the terminal:\n{:?}",
        transcript.0
    );
    assert!(
        !transcript.0.contains(ENTER_ALTERNATE_SCREEN),
        "nothing is drawn"
    );
    assert_eq!(
        std::fs::read_to_string(home.path().join("stdout")).expect("read redirected stdout"),
        ""
    );
    transcript.assert_modes_unchanged();
}

#[test]
fn with_stdin_redirected_it_refuses_without_touching_the_terminal() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(r#""$WHETSTONE" < /dev/null"#, &[("HOME", home.path())]);

    let transcript = Transcript(session.exp_eof().expect("shell exits"));

    transcript.assert_exit_code(1);
    assert!(
        transcript.0.contains(
            "error: whetstone needs an interactive terminal, but stdin is not a terminal"
        ),
        "the refusal is on stderr, in the terminal:\n{:?}",
        transcript.0
    );
    assert!(
        !transcript.0.contains(ENTER_ALTERNATE_SCREEN),
        "nothing is drawn"
    );
    transcript.assert_modes_unchanged();
}

#[test]
fn with_no_color_nothing_is_coloured_and_focus_stays_bold() {
    let home = TempDir::new().expect("create a temporary home");
    let mut session = start(LAUNCH_WITHOUT_COLOR, &[("HOME", home.path())]);

    let transcript = quit_with_q(&mut session);

    transcript.assert_exit_code(0);
    let styles = transcript.drawn_styles();
    assert!(
        styles.contains(&"1"),
        "the focused action is drawn bold: {styles:?}"
    );
    // crossterm 0.29 writes a colour as a bare `CSI m` under NO_COLOR, which
    // resets every attribute, bold included.
    assert!(
        !styles
            .iter()
            .any(|parameters| parameters.split(';').all(str::is_empty)),
        "no bare reset clears the attributes: {styles:?}"
    );
    assert!(
        !styles.iter().any(|parameters| sets_a_color(parameters)),
        "no colour is set: {styles:?}"
    );
}

/// Whether SGR `parameters` set a foreground, background or underline
/// colour. 39, 49 and 59 return to the terminal's own colours.
pub(super) fn sets_a_color(parameters: &str) -> bool {
    parameters
        .split(';')
        .filter_map(|parameter| parameter.parse::<u16>().ok())
        .any(|parameter| matches!(parameter, 30..=38 | 40..=48 | 58 | 90..=97 | 100..=107))
}
