//! The `questions` setting: how many questions a Practice asks for, a
//! top-level key in the global config holding a whole number from 3 to 5,
//! by default 4. Settings shows it in its Questions row.

use std::error::Error as _;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use whetstone::{Environment, Settings};

use crate::session::{NO_COLOR, Session, Setup};

/// Writes `contents` to `config.toml` in `dir` and loads it, with every
/// cause of a failure after its message.
fn load(dir: &Path, contents: &str) -> Result<Settings, String> {
    fs::write(dir.join("config.toml"), contents).expect("write the config file");
    Settings::load(dir, &Environment::default()).map_err(|error| {
        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            message.push_str(": ");
            message.push_str(&cause.to_string());
            source = cause.source();
        }
        message
    })
}

/// The Questions row on Settings, opened from home, with `config` as the
/// global config.
fn questions_row(config: Option<&str>) -> String {
    let mut session = Session::start(
        Setup {
            config,
            variables: NO_COLOR,
            ..Setup::default()
        },
        80,
        24,
    );
    session.press("j <Enter>");
    session.row_with("Questions")
}

#[test]
fn without_the_setting_a_practice_asks_for_4_questions() {
    assert_eq!(questions_row(None), "Questions             4 (default)");
    assert_eq!(
        questions_row(Some("colours = \"auto\"\n")),
        "Questions             4 (default)"
    );
}

#[test]
fn the_setting_chooses_from_3_to_5_questions() {
    for count in ["3", "4", "5"] {
        assert_eq!(
            questions_row(Some(&format!("questions = {count}\n"))),
            format!("Questions             {count}")
        );
    }
}

#[test]
fn a_questions_value_that_is_not_a_whole_number_from_3_to_5_is_an_error_naming_the_file_and_the_key()
 {
    for value in ["2", "6", "0", "-1", "4.0", "\"4\""] {
        let dir = TempDir::new().expect("create a temporary config directory");
        let config = format!("colours = \"auto\"\nquestions = {value}\n");

        let error = load(dir.path(), &config).expect_err("out of range or not an integer");

        let path = dir.path().join("config.toml");
        assert!(
            error.starts_with(&format!(
                "the config file {} is not valid: ",
                path.display()
            )),
            "{value}: {error}"
        );
        assert!(error.contains("line 2"), "says where, for {value}: {error}");
        assert!(
            error.contains("`questions` must be a whole number from 3 to 5"),
            "names the key and what it allows, for {value}: {error}"
        );
    }
}

#[test]
fn questions_is_a_top_level_key_never_one_under_a_role() {
    let dir = TempDir::new().expect("create a temporary config directory");

    let error = load(dir.path(), "[generator]\nquestions = 4\n").expect_err("an unknown key");

    assert!(error.contains("questions"), "{error}");
}

#[test]
fn settings_says_what_the_setting_does_and_how_to_change_it() {
    let mut session = Session::start(
        Setup {
            config: Some("questions = 5\n"),
            variables: NO_COLOR,
            ..Setup::default()
        },
        120,
        32,
    );
    session.press("j <Enter>");
    session.press("j j j j j j");

    assert_eq!(session.row_with("Questions"), "> Questions             5");
    let text = session.text();
    assert!(
        text.contains("How many questions a Practice asks for, from 3 to 5. The default is 4."),
        "{text}"
    );
    assert!(text.contains("In config.toml: questions = 5"), "{text}");
}

/// The README, where the config file and its defaults are documented.
const README: &str = include_str!("../../../../README.md");

#[test]
fn the_readme_documents_the_default() {
    let (_, after) = README
        .split_once("## Configure it")
        .expect("the README has a section on config");
    let documented = after
        .split_once("```toml\n")
        .and_then(|(_, block)| block.split_once("```"))
        .map(|(toml, _)| toml)
        .expect("the section shows a config file");

    assert!(
        documented.lines().any(|line| line == "questions = 4"),
        "{documented}"
    );
    assert_eq!(
        questions_row(Some(documented)),
        "Questions             4",
        "the README's file sets the default"
    );
}
