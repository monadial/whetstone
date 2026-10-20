//! Settings: read-only, it shows the provider, the Jev model, the generator
//! model and the colours from the global config, and whether
//! `OPENROUTER_API_KEY` is set, never its value.

use whetstone::Flow;

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
    assert_eq!(
        session.row_with("Colours"),
        "Colours          auto (16 detected) (default)"
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
colours = "24-bit"

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
            ..Setup::default()
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
    assert_eq!(session.row_with("Colours"), "Colours          24-bit");
}

#[test]
fn settings_names_the_default_for_whatever_the_config_file_leaves_out() {
    let session = settings(
        Setup {
            config: Some("[generator]\nmodel = \"anthropic/claude-haiku-5.5\"\n"),
            variables: NO_COLOR,
            ..Setup::default()
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
    assert_eq!(
        session.row_with("Colours"),
        "Colours          auto (16 detected) (default)"
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
            ..Setup::default()
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
            ..Setup::default()
        },
        80,
        24,
    );

    insta::assert_debug_snapshot!("settings_with_config_and_key_80x24", session.frame());
}

/// The Colours row, from a config file holding `config` if any, in a
/// terminal whose environment is `variables`.
fn colours_row(config: Option<&str>, variables: &[(&str, &str)]) -> String {
    settings(
        Setup {
            config,
            variables,
            ..Setup::default()
        },
        80,
        24,
    )
    .row_with("Colours")
}

#[test]
fn the_colours_row_shows_a_level_the_config_file_sets() {
    for (value, shown) in [
        ("24-bit", "24-bit"),
        ("256", "256"),
        ("16", "16"),
        ("none", "none"),
    ] {
        let config = format!("colours = \"{value}\"\n");
        // What the terminal reports makes no difference.
        assert_eq!(
            colours_row(Some(&config), &[("COLORTERM", "truecolor")]),
            format!("Colours          {shown}"),
            "{config:?}"
        );
    }
}

#[test]
fn with_colours_auto_the_row_shows_the_level_detected() {
    for (variables, detected) in [
        (&[("COLORTERM", "truecolor")][..], "24-bit"),
        (&[("COLORTERM", "24bit"), ("TERM", "xterm")], "24-bit"),
        (&[("TERM", "xterm-256color")], "256"),
        (&[("TERM", "xterm")], "16"),
        (&[], "16"),
    ] {
        assert_eq!(
            colours_row(None, variables),
            format!("Colours          auto ({detected} detected) (default)"),
            "with {variables:?}"
        );
        assert_eq!(
            colours_row(Some("colours = \"auto\"\n"), variables),
            format!("Colours          auto ({detected} detected)"),
            "set in the config file, with {variables:?}"
        );
    }
}

/// Settings with the Colours row focused.
fn colours_focused(config: Option<&str>, variables: &[(&str, &str)]) -> Session {
    let mut session = settings(
        Setup {
            config,
            variables,
            ..Setup::default()
        },
        80,
        24,
    );
    session.press("j j j j");
    assert!(
        session.row_with("Colours").starts_with("> Colours"),
        "{}",
        session.text()
    );
    session
}

#[test]
fn the_focused_colours_row_says_what_it_does_and_how_to_change_it() {
    let session = colours_focused(Some("colours = \"256\"\n"), &[]);

    let text = session.text();
    assert!(text.contains("How much colour"), "{text}");
    assert!(text.contains("NO_COLOR"), "says NO_COLOR wins: {text}");
    assert!(text.contains("In config.toml: colours = \"256\""), "{text}");
}

#[test]
fn when_no_color_overrides_the_colours_setting_settings_says_so() {
    for (config, variables) in [
        (Some("colours = \"24-bit\"\n"), NO_COLOR),
        (None, &[("NO_COLOR", "1"), ("COLORTERM", "truecolor")][..]),
    ] {
        let session = colours_focused(config, variables);

        assert!(
            session
                .text()
                .contains("NO_COLOR is set, so nothing is coloured"),
            "{config:?} with {variables:?}:\n{}",
            session.text()
        );
    }
    // Where nothing is overridden, it does not say so.
    for (config, variables) in [
        (Some("colours = \"none\"\n"), NO_COLOR),
        (Some("colours = \"24-bit\"\n"), &[][..]),
        (None, &[("NO_COLOR", ""), ("COLORTERM", "truecolor")]),
    ] {
        let session = colours_focused(config, variables);

        assert!(
            !session.text().contains("NO_COLOR is set"),
            "{config:?} with {variables:?}:\n{}",
            session.text()
        );
    }
}

#[test]
fn the_colours_row_cannot_be_changed_from_settings() {
    let mut session = colours_focused(None, NO_COLOR);
    let before = session.text();

    assert_eq!(session.press("<Enter> <Space>"), Flow::Continue);
    assert_eq!(session.text(), before);
}

#[test]
fn settings_with_colours_focused_at_120x32() {
    let mut session = settings(
        Setup {
            config: Some("colours = \"none\"\n"),
            ..Setup::default()
        },
        120,
        32,
    );
    session.press("j j j j");

    insta::assert_debug_snapshot!("settings_with_colours_focused_120x32", session.frame());
}

#[test]
fn settings_with_colours_focused_at_80x24() {
    let mut session = settings(
        Setup {
            config: Some("colours = \"none\"\n"),
            ..Setup::default()
        },
        80,
        24,
    );
    session.press("j j j j");

    insta::assert_debug_snapshot!("settings_with_colours_focused_80x24", session.frame());
}
