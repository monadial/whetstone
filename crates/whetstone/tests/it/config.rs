//! The global config is `config.toml` in the XDG config directory's
//! `whetstone` folder. A file that exists but cannot be used is an error that
//! names it, never silently replaced by the defaults.

use std::error::Error as _;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use whetstone::{Environment, Settings};

use crate::session::{NO_COLOR, Session, Setup};

/// Writes `contents` to `config.toml` in `dir` and loads it.
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

#[test]
fn xdg_config_home_chooses_where_the_config_file_is() {
    let xdg = TempDir::new().expect("create a temporary config home");
    let xdg_path = xdg.path().to_str().expect("a UTF-8 path");
    let mut session = Session::start(
        Setup {
            config: Some("[jev]\nmodel = \"typesafe/jev-1.13\"\n"),
            variables: &[("NO_COLOR", "1"), ("XDG_CONFIG_HOME", xdg_path)],
            ..Setup::default()
        },
        120,
        32,
    );
    session.press("j <Enter>");

    let shown = format!("Read from {xdg_path}/whetstone/config.toml.");
    assert!(session.text().contains(&shown), "{}", session.text());
    assert_eq!(
        session.row_with("Jev model"),
        "Jev model        typesafe/jev-1.13"
    );
}

#[test]
fn a_config_file_that_is_not_toml_is_an_error_naming_the_file() {
    let dir = TempDir::new().expect("create a temporary config directory");

    let error = load(dir.path(), "provider = openrouter\n").expect_err("invalid TOML");

    let path = dir.path().join("config.toml");
    assert!(
        error.starts_with(&format!(
            "the config file {} is not valid: ",
            path.display()
        )),
        "{error}"
    );
    assert!(error.contains("line 1"), "says where: {error}");
}

#[test]
fn an_unknown_setting_is_an_error_rather_than_ignored() {
    let dir = TempDir::new().expect("create a temporary config directory");

    let error = load(dir.path(), "[jev]\nmodle = \"typesafe/jev-1.13\"\n").expect_err("a typo");

    assert!(error.contains("unknown field `modle`"), "{error}");
}

#[test]
fn an_unknown_provider_is_an_error() {
    let dir = TempDir::new().expect("create a temporary config directory");

    let error = load(dir.path(), "provider = \"elsewhere\"\n").expect_err("not OpenRouter");

    assert!(error.contains("unknown variant `elsewhere`"), "{error}");
}

#[test]
fn an_unknown_colours_value_is_an_error_naming_the_file_and_the_value() {
    let dir = TempDir::new().expect("create a temporary config directory");

    let error = load(dir.path(), "colours = \"truecolour\"\n").expect_err("not a colour level");

    let path = dir.path().join("config.toml");
    assert!(
        error.starts_with(&format!(
            "the config file {} is not valid: ",
            path.display()
        )),
        "{error}"
    );
    assert!(error.contains("line 1"), "says where: {error}");
    assert!(
        error.contains(
            "unknown variant `truecolour`, expected one of `auto`, `24-bit`, `256`, `16`, `none`"
        ),
        "names the value and the ones allowed: {error}"
    );
}

#[test]
fn a_colours_value_that_is_not_a_string_is_an_error() {
    let dir = TempDir::new().expect("create a temporary config directory");

    let error = load(dir.path(), "colours = 256\n").expect_err("a number, not a string");

    let path = dir.path().join("config.toml");
    assert!(
        error.starts_with(&format!(
            "the config file {} is not valid: ",
            path.display()
        )),
        "{error}"
    );
    assert!(error.contains("line 1"), "says where: {error}");
}

#[test]
fn a_config_path_that_cannot_be_read_is_an_error_naming_it() {
    let dir = TempDir::new().expect("create a temporary config directory");
    fs::create_dir(dir.path().join("config.toml")).expect("a directory in the file's place");

    let error = Settings::load(dir.path(), &Environment::default())
        .expect_err("a directory is not a file")
        .to_string();

    assert_eq!(
        error,
        format!(
            "cannot read the config file {}",
            dir.path().join("config.toml").display()
        )
    );
}

#[test]
fn an_empty_config_file_means_every_default() {
    let dir = TempDir::new().expect("create a temporary config directory");

    assert!(load(dir.path(), "").is_ok());
}

/// The README, where the config file and its defaults are documented.
const README: &str = include_str!("../../../../README.md");

/// The values Settings shows, without saying which are defaults.
fn values(setup: Setup<'_>) -> Vec<String> {
    let mut session = Session::start(setup, 80, 24);
    session.press("j <Enter>");
    ["Provider", "Jev model", "Generator model"]
        .map(|setting| session.row_with(setting).replace(" (default)", ""))
        .to_vec()
}

#[test]
fn the_readme_documents_the_defaults() {
    let (_, after) = README
        .split_once("## Configure it")
        .expect("the README has a section on config");
    let documented = after
        .split_once("```toml\n")
        .and_then(|(_, block)| block.split_once("```"))
        .map(|(toml, _)| toml)
        .expect("the section shows a config file");

    assert_eq!(
        values(Setup {
            config: Some(documented),
            variables: NO_COLOR,
            ..Setup::default()
        }),
        values(Setup {
            variables: NO_COLOR,
            ..Setup::default()
        })
    );
}
