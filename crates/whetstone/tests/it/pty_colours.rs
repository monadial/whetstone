//! The Colours setting in a real terminal: what the config file asks for is
//! what reaches the terminal, whatever `TERM` and `COLORTERM` say.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::pty::{LAUNCH, Transcript, quit_with_q, sets_a_color, start};

/// Writes `config.toml` with `contents` where Whetstone looks for it under
/// `home`, then runs Whetstone and quits it with `q`. The terminal is an
/// `xterm-256color` without `COLORTERM`, as over SSH.
fn run_with_config(home: &Path, contents: &str) -> Transcript {
    let config = home.join(".config/whetstone");
    fs::create_dir_all(&config).expect("create the config directory");
    fs::write(config.join("config.toml"), contents).expect("write the config file");
    let mut session = start(LAUNCH, &[("HOME", home)]);
    quit_with_q(&mut session)
}

#[test]
fn with_colours_none_in_the_config_nothing_is_coloured_and_focus_stays_bold() {
    let home = TempDir::new().expect("create a temporary home");

    let transcript = run_with_config(home.path(), "colours = \"none\"\n");

    transcript.assert_exit_code(0);
    let styles = transcript.drawn_styles();
    assert!(
        styles.contains(&"1"),
        "the focused action is drawn bold: {styles:?}"
    );
    assert!(
        !styles
            .iter()
            .any(|parameters| parameters.split(';').all(str::is_empty)),
        "no bare reset clears the attributes: {styles:?}"
    );
    assert!(
        !styles.iter().any(|parameters| sets_a_color(parameters)),
        "no colour is set, though TERM names 256 colours: {styles:?}"
    );
}

#[test]
fn with_colours_24_bit_in_the_config_24_bit_colour_is_drawn_without_colorterm() {
    let home = TempDir::new().expect("create a temporary home");

    let transcript = run_with_config(home.path(), "colours = \"24-bit\"\n");

    transcript.assert_exit_code(0);
    let styles = transcript.drawn_styles();
    assert!(
        styles
            .iter()
            .any(|parameters| parameters.contains("38;2;") || parameters.contains("48;2;")),
        "24-bit colour is drawn: {styles:?}"
    );
    assert!(
        !styles
            .iter()
            .any(|parameters| parameters.contains("38;5;") || parameters.contains("48;5;")),
        "no 256-colour index is drawn, though TERM names 256 colours: {styles:?}"
    );
}
