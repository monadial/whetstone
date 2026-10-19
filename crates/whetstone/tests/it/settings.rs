//! Settings: read-only, it shows the provider, the Jev model and the
//! generator model from the global config, and whether `OPENROUTER_API_KEY`
//! is set, never its value.

use crate::session::{NO_COLOR, Session, Setup};

/// Opens Settings from home.
fn settings(setup: Setup<'_>, width: u16, height: u16) -> Session {
    let mut session = Session::start(setup, width, height);
    session.press("j <Enter>");
    session
}

#[test]
fn without_a_config_file_settings_shows_the_defaults_and_says_so() {
    let session = settings(
        Setup {
            variables: NO_COLOR,
            ..Setup::default()
        },
        80,
        24,
    );

    assert!(
        session
            .text()
            .contains("No config found at ~/.config/whetstone/config.toml"),
        "{}",
        session.text()
    );
    assert_eq!(
        session.row_with("Provider"),
        "> Provider         OpenRouter (default)"
    );
    assert_eq!(
        session.row_with("Jev model"),
        "Jev model        typesafe/jev-1.14 (default)"
    );
    assert_eq!(
        session.row_with("Generator model"),
        "Generator model  anthropic/claude-sonnet-5.5 (default)"
    );
    assert_eq!(
        session.row_with("OpenRouter key"),
        "OpenRouter key   not set (OPENROUTER_API_KEY)"
    );
}

#[test]
fn settings_without_config_or_key_at_120x32() {
    let session = settings(
        Setup {
            variables: NO_COLOR,
            ..Setup::default()
        },
        120,
        32,
    );

    insta::assert_debug_snapshot!("settings_without_config_or_key_120x32", session.frame());
}

#[test]
fn settings_without_config_or_key_at_80x24() {
    let session = settings(
        Setup {
            variables: NO_COLOR,
            ..Setup::default()
        },
        80,
        24,
    );

    insta::assert_debug_snapshot!("settings_without_config_or_key_80x24", session.frame());
}

/// A config file that sets everything.
const FULL_CONFIG: &str = r#"
provider = "openrouter"

[jev]
model = "typesafe/jev-1.13"

[generator]
model = "anthropic/claude-opus-5.5"
"#;

/// A key value that must never reach the screen.
const SECRET: &str = "sk-or-v1-never-shown-0123456789";

#[test]
fn with_a_config_file_settings_shows_its_values_and_where_they_come_from() {
    let session = settings(
        Setup {
            config: Some(FULL_CONFIG),
            variables: NO_COLOR,
        },
        80,
        24,
    );

    assert!(
        session
            .text()
            .contains("Read from ~/.config/whetstone/config.toml."),
        "{}",
        session.text()
    );
    assert_eq!(
        session.row_with("Provider"),
        "> Provider         OpenRouter"
    );
    assert_eq!(
        session.row_with("Jev model"),
        "Jev model        typesafe/jev-1.13"
    );
    assert_eq!(
        session.row_with("Generator model"),
        "Generator model  anthropic/claude-opus-5.5"
    );
}

#[test]
fn settings_names_the_default_for_whatever_the_config_file_leaves_out() {
    let session = settings(
        Setup {
            config: Some("[generator]\nmodel = \"anthropic/claude-haiku-5.5\"\n"),
            variables: NO_COLOR,
        },
        80,
        24,
    );

    assert_eq!(
        session.row_with("Provider"),
        "> Provider         OpenRouter (default)"
    );
    assert_eq!(
        session.row_with("Jev model"),
        "Jev model        typesafe/jev-1.14 (default)"
    );
    assert_eq!(
        session.row_with("Generator model"),
        "Generator model  anthropic/claude-haiku-5.5"
    );
}

#[test]
fn with_the_key_set_settings_says_so_and_never_shows_its_value() {
    let mut session = settings(
        Setup {
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", SECRET)],
            ..Setup::default()
        },
        120,
        32,
    );

    assert_eq!(
        session.row_with("OpenRouter key"),
        "OpenRouter key   set (OPENROUTER_API_KEY)"
    );
    // Not on any control, nor in key help or the palette over Settings.
    for keys in ["j", "j", "j", "j", "?", "<Esc> :"] {
        session.press(keys);
        assert!(!session.text().contains(SECRET), "{}", session.text());
        assert!(!session.text().contains("sk-or"), "{}", session.text());
    }
}

#[test]
fn an_empty_key_variable_counts_as_not_set() {
    let session = settings(
        Setup {
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", "")],
            ..Setup::default()
        },
        80,
        24,
    );

    assert_eq!(
        session.row_with("OpenRouter key"),
        "OpenRouter key   not set (OPENROUTER_API_KEY)"
    );
}

#[test]
fn settings_with_config_and_key_at_120x32() {
    let session = settings(
        Setup {
            config: Some(FULL_CONFIG),
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", SECRET)],
        },
        120,
        32,
    );

    insta::assert_debug_snapshot!("settings_with_config_and_key_120x32", session.frame());
}

#[test]
fn settings_with_config_and_key_at_80x24() {
    let session = settings(
        Setup {
            config: Some(FULL_CONFIG),
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", SECRET)],
        },
        80,
        24,
    );

    insta::assert_debug_snapshot!("settings_with_config_and_key_80x24", session.frame());
}
