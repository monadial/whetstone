//! The `[cut_offs]` table: the assessment policy's cut-offs for each job,
//! which only the learner's global config can override. A job it leaves out
//! keeps policy 1's: 0.50 and 0.20 for the giveaway check, and 0.80 and 0.20
//! for every other job. Settings shows each job's, marked default or from
//! the config, and says when outcomes aren't comparable with policy 1's.

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

/// Settings, opened from home, with `config` as the global config, in a
/// `width` by `height` terminal.
fn settings(config: Option<&str>, width: u16, height: u16) -> Session {
    let mut session = Session::start(
        Setup {
            config,
            variables: NO_COLOR,
            ..Setup::default()
        },
        width,
        height,
    );
    session.press("j <Enter>");
    session
}

/// The screen's text as one line of words, so a phrase is found wherever
/// it wraps.
fn words(session: &Session) -> String {
    session
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each job's name, as Settings lists the cut-offs the config sets for it,
/// with its key in `[cut_offs]`.
const JOBS: [(&str, &str); 7] = [
    ("relevance gate", "relevance_gate"),
    ("question check", "question_check"),
    ("giveaway check", "giveaway_check"),
    ("option rating", "option_rating"),
    ("rationale and hint checks", "rationale_and_hint_checks"),
    ("reasoning criterion", "reasoning_criterion"),
    ("explanation check", "explanation_check"),
];

/// The Cut-offs row when every job has policy 1's.
const EVERY_JOB_AT_POLICY_1: &str =
    "Cut-offs              upper 0.8, lower 0.2 for every other job (default)";

/// The line under it that lists the giveaway check's own default.
const GIVEAWAY_CHECK_AT_POLICY_1: &str = "giveaway check: upper 0.5, lower 0.2 (default)";

/// What Settings says when any cut-off isn't policy 1's.
const NOT_COMPARABLE: &str =
    "Cut-offs changed in your config: outcomes aren't comparable with policy 1";

/// A config that lowers the question check's upper cut-off.
const LOWER_QUESTION_CHECK: &str = "[cut_offs]\nquestion_check = { upper = 0.55, lower = 0.20 }\n";

#[test]
fn without_the_table_every_job_shows_policy_1_s_cut_offs_as_the_default() {
    for config in [None, Some("questions = 4\n"), Some("[cut_offs]\n")] {
        let session = settings(config, 80, 24);

        assert_eq!(
            session.row_with("Cut-offs"),
            EVERY_JOB_AT_POLICY_1,
            "{config:?}"
        );
        let text = words(&session);
        assert!(
            text.contains(GIVEAWAY_CHECK_AT_POLICY_1),
            "{config:?}: {text}"
        );
        assert!(!text.contains("from your config"), "{config:?}: {text}");
        assert!(!text.contains("Cut-offs changed"), "{config:?}: {text}");
    }
}

#[test]
fn a_job_the_table_lists_takes_its_cut_offs_and_every_other_keeps_policy_1_s() {
    let session = settings(Some(LOWER_QUESTION_CHECK), 120, 32);

    assert_eq!(session.row_with("Cut-offs"), EVERY_JOB_AT_POLICY_1);
    let text = words(&session);
    assert!(
        text.contains("question check: upper 0.55, lower 0.2 (from your config)"),
        "{text}"
    );
    for (name, _) in JOBS
        .into_iter()
        .filter(|&(name, _)| !["question check", "giveaway check"].contains(&name))
    {
        assert!(!text.contains(&format!("{name}:")), "{name}: {text}");
    }
    assert!(text.contains(NOT_COMPARABLE), "{text}");
}

#[test]
fn a_learner_s_question_check_lists_the_giveaway_check_s_default_beside_it() {
    let session = settings(Some(LOWER_QUESTION_CHECK), 120, 32);

    let rows: Vec<String> = session
        .text()
        .lines()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let question_check = rows
        .iter()
        .position(|row| row == "question check: upper 0.55, lower 0.2 (from your config)")
        .unwrap_or_else(|| panic!("the question check's row: {rows:#?}"));
    assert_eq!(rows[question_check + 1], GIVEAWAY_CHECK_AT_POLICY_1);
}

#[test]
fn the_learner_s_giveaway_check_is_listed_from_their_config() {
    let session = settings(
        Some("[cut_offs]\ngiveaway_check = { upper = 0.6, lower = 0.25 }\n"),
        120,
        32,
    );

    assert_eq!(session.row_with("Cut-offs"), EVERY_JOB_AT_POLICY_1);
    let text = words(&session);
    assert!(
        text.contains("giveaway check: upper 0.6, lower 0.25 (from your config)"),
        "{text}"
    );
    assert!(!text.contains(GIVEAWAY_CHECK_AT_POLICY_1), "{text}");
    assert!(text.contains(NOT_COMPARABLE), "{text}");
}

#[test]
fn every_job_has_an_entry_and_a_whole_0_or_1_is_a_number() {
    let entries = JOBS
        .map(|(_, key)| format!("{key} = {{ upper = 1, lower = 0 }}"))
        .join("\n");
    let session = settings(Some(&format!("[cut_offs]\n{entries}\n")), 120, 32);

    assert_eq!(
        session.row_with("Cut-offs"),
        "Cut-offs              from your config for every job"
    );
    let text = words(&session);
    for (name, _) in JOBS {
        assert!(
            text.contains(&format!("{name}: upper 1, lower 0 (from your config)")),
            "{name}: {text}"
        );
    }
    assert!(text.contains(NOT_COMPARABLE), "{text}");
}

#[test]
fn an_entry_at_policy_1_s_values_is_the_default_and_outcomes_stay_comparable() {
    let session = settings(
        Some(
            "[cut_offs]\noption_rating = { upper = 0.80, lower = 0.20 }\n\
             giveaway_check = { upper = 0.50, lower = 0.20 }\n",
        ),
        120,
        32,
    );

    assert_eq!(session.row_with("Cut-offs"), EVERY_JOB_AT_POLICY_1);
    let text = words(&session);
    assert!(text.contains(GIVEAWAY_CHECK_AT_POLICY_1), "{text}");
    assert!(!text.contains("from your config"), "{text}");
    assert!(!text.contains("Cut-offs changed"), "{text}");
}

#[test]
fn a_cut_off_that_is_not_a_number_from_0_to_1_below_its_upper_is_an_error_naming_the_file_and_the_key()
 {
    for (entry, says) in [
        (
            "question_check = { upper = 1.5, lower = 0.2 }",
            "`cut_offs.question_check`: `upper` must be a number from 0 to 1",
        ),
        (
            "giveaway_check = { upper = 1.5, lower = 0.2 }",
            "`cut_offs.giveaway_check`: `upper` must be a number from 0 to 1",
        ),
        (
            "giveaway_check = { upper = 0.5, lower = 0.5 }",
            "`cut_offs.giveaway_check`: `lower` must be below `upper`",
        ),
        (
            "giveaway_check = { lower = 0.2 }",
            "`cut_offs.giveaway_check` needs both `upper` and `lower`",
        ),
        (
            "option_rating = { upper = 0.8, lower = -0.1 }",
            "`cut_offs.option_rating`: `lower` must be a number from 0 to 1",
        ),
        (
            "relevance_gate = { upper = \"0.8\", lower = 0.2 }",
            "`cut_offs.relevance_gate`: `upper` must be a number from 0 to 1",
        ),
        (
            "explanation_check = { upper = 0.8, lower = 2 }",
            "`cut_offs.explanation_check`: `lower` must be a number from 0 to 1",
        ),
        (
            "reasoning_criterion = { upper = 0.5, lower = 0.5 }",
            "`cut_offs.reasoning_criterion`: `lower` must be below `upper`",
        ),
        (
            "rationale_and_hint_checks = { upper = 0.2, lower = 0.8 }",
            "`cut_offs.rationale_and_hint_checks`: `lower` must be below `upper`",
        ),
        (
            "question_check = { upper = 0.8 }",
            "`cut_offs.question_check` needs both `upper` and `lower`",
        ),
        (
            "question_check = { upper = 0.8, lower = 0.2, middle = 0.5 }",
            "`cut_offs.question_check` takes only `upper` and `lower`, not `middle`",
        ),
        (
            "question_check = 0.55",
            "`cut_offs.question_check` needs both `upper` and `lower`",
        ),
        (
            "questions_check = { upper = 0.8, lower = 0.2 }",
            "`cut_offs.questions_check` is not a job",
        ),
    ] {
        let dir = TempDir::new().expect("create a temporary config directory");
        let config = format!("colours = \"auto\"\n[cut_offs]\n{entry}\n");

        let error = load(dir.path(), &config).expect_err("an invalid cut-off");

        let path = dir.path().join("config.toml");
        assert!(
            error.starts_with(&format!(
                "the config file {} is not valid: ",
                path.display()
            )),
            "{entry}: {error}"
        );
        assert!(error.contains("line 3"), "says where, for {entry}: {error}");
        assert!(error.contains(says), "{entry}: {error}");
    }
}

#[test]
fn cut_offs_is_a_table_of_its_own() {
    for config in [
        "cut_offs = 0.8\n",
        "[jev]\ncut_offs = { question_check = { upper = 0.8, lower = 0.2 } }\n",
    ] {
        let dir = TempDir::new().expect("create a temporary config directory");

        let error = load(dir.path(), config).expect_err("not the global table");

        assert!(error.contains("cut_offs"), "{config:?}: {error}");
    }
}

#[test]
fn settings_says_what_the_cut_offs_do_and_how_to_change_them() {
    for (config, how) in [
        (
            None,
            "In config.toml, under [cut_offs]: question_check = { upper = 0.8, lower = 0.2 }",
        ),
        (
            Some("[cut_offs]\nreasoning_criterion = { upper = 0.7, lower = 0.25 }\n"),
            "In config.toml, under [cut_offs]: reasoning_criterion = { upper = 0.7, lower = 0.25 }",
        ),
    ] {
        let mut session = settings(config, 120, 32);

        // Past the eight settings above it.
        session.press("j j j j j j j j");

        assert!(
            session.row_with("Cut-offs").starts_with("> Cut-offs"),
            "{}",
            session.text()
        );
        let text = words(&session);
        assert!(
            text.contains("at or above upper is clear yes, at or below lower is clear no"),
            "{text}"
        );
        assert!(
            text.contains(
                "The jobs are relevance_gate, question_check, giveaway_check, option_rating, \
                 rationale_and_hint_checks, reasoning_criterion, explanation_check."
            ),
            "{text}"
        );
        assert!(
            text.contains("A repository's config can't set them."),
            "{text}"
        );
        assert!(text.contains(how), "{config:?}: {text}");
    }
}

#[test]
fn the_policy_row_says_its_cut_offs_can_be_overridden() {
    let mut session = settings(None, 120, 32);

    session.press("j j j j j j j");

    let text = words(&session);
    assert!(
        text.contains("your config can override any job's under [cut_offs]"),
        "{text}"
    );
    assert!(!text.contains("can't be configured"), "{text}");
}

#[test]
fn settings_with_policy_1_s_cut_offs_at_120x32() {
    insta::assert_debug_snapshot!(
        "settings_with_policy_1_s_cut_offs_120x32",
        settings(None, 120, 32).frame()
    );
}

#[test]
fn settings_with_policy_1_s_cut_offs_at_80x24() {
    insta::assert_debug_snapshot!(
        "settings_with_policy_1_s_cut_offs_80x24",
        settings(None, 80, 24).frame()
    );
}

#[test]
fn settings_with_a_cut_off_from_the_config_at_120x32() {
    insta::assert_debug_snapshot!(
        "settings_with_a_cut_off_from_the_config_120x32",
        settings(Some(LOWER_QUESTION_CHECK), 120, 32).frame()
    );
}

#[test]
fn settings_with_a_cut_off_from_the_config_at_80x24() {
    insta::assert_debug_snapshot!(
        "settings_with_a_cut_off_from_the_config_80x24",
        settings(Some(LOWER_QUESTION_CHECK), 80, 24).frame()
    );
}

/// The README, where the config file and its defaults are documented.
const README: &str = include_str!("../../../../README.md");

#[test]
fn the_readme_documents_the_table_with_policy_1_s_cut_offs() {
    let (_, after) = README
        .split_once("## Configure it")
        .expect("the README has a section on config");
    let documented = after
        .split_once("```toml\n")
        .and_then(|(_, block)| block.split_once("```"))
        .map(|(toml, _)| toml)
        .expect("the section shows a config file");

    assert!(
        documented.lines().any(|line| line == "[cut_offs]"),
        "{documented}"
    );
    for (_, key) in JOBS {
        let upper = if key == "giveaway_check" {
            "0.50"
        } else {
            "0.80"
        };
        assert!(
            documented
                .lines()
                .any(|line| line == format!("{key} = {{ upper = {upper}, lower = 0.20 }}")),
            "{key}: {documented}"
        );
    }
    let session = settings(Some(documented), 120, 32);
    assert_eq!(
        session.row_with("Cut-offs"),
        EVERY_JOB_AT_POLICY_1,
        "the README's file sets policy 1's"
    );
    assert!(!words(&session).contains("from your config"));
}
