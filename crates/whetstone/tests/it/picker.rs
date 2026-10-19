//! The area and change picker: "Practice on the showcase project" opens it.
//! The learner chooses areas, all selected at first, and one change from the
//! showcase project's curated history. "Generate questions" waits for Jev.

use whetstone::Flow;
use whetstone_core::{Area, ShowcaseProject};

use crate::session::{NO_COLOR, Session, Setup};

#[test]
fn practice_on_the_showcase_project_opens_the_picker_from_home() {
    let mut session = Session::plain(80, 24);

    session.press("<Enter>");

    assert_eq!(session.rows()[1].trim(), "Practice on the showcase project");
}

/// The heading above the areas.
const AREAS: &str = "Areas the questions focus on";

/// The rows under `heading`, trimmed, up to the next blank row.
fn section(session: &Session, heading: &str) -> Vec<String> {
    let rows = session.rows();
    let start = rows.iter().position(|row| row.trim() == heading);
    assert!(
        start.is_some(),
        "no {heading:?} heading:\n{}",
        session.text()
    );
    rows.iter()
        .skip(start.unwrap_or_default() + 1)
        .map(|row| row.trim().to_owned())
        .take_while(|row| !row.is_empty())
        .collect()
}

#[test]
fn every_area_the_showcase_project_declares_starts_selected() {
    let mut session = Session::plain(80, 24);

    session.press("<Enter>");

    assert_eq!(
        section(&session, AREAS),
        [
            "> (x) domain",
            "(x) observability",
            "(x) scalability",
            "(x) infrastructure",
            "(x) system design",
        ]
    );
}

/// The picker in an 80x24 terminal without colour, over `showcase`.
fn picker_over(showcase: &ShowcaseProject) -> Session {
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            showcase: Some(showcase),
            ..Setup::default()
        },
        80,
        24,
    );
    session.press("<Enter>");
    session
}

#[test]
fn the_areas_are_the_ones_the_showcase_project_declares() {
    let showcase = ShowcaseProject::new(
        vec![Area::new("security"), Area::new("performance")],
        whetstone::embedded_showcase().changes().to_vec(),
    );

    let session = picker_over(&showcase);

    assert_eq!(
        section(&session, AREAS),
        ["> (x) security", "(x) performance"]
    );
}

#[test]
fn a_showcase_project_that_declares_no_areas_offers_the_five_defaults_all_selected() {
    let showcase = ShowcaseProject::new(
        Vec::new(),
        whetstone::embedded_showcase().changes().to_vec(),
    );

    let session = picker_over(&showcase);

    assert_eq!(
        section(&session, AREAS),
        [
            "> (x) domain",
            "(x) observability",
            "(x) scalability",
            "(x) infrastructure",
            "(x) system design",
        ]
    );
}

#[test]
fn space_and_enter_toggle_the_focused_area() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    session.press("<Space>");
    assert_eq!(section(&session, AREAS)[0], "> ( ) domain");

    session.press("j <Space>");
    assert_eq!(
        section(&session, AREAS)[..2],
        ["( ) domain", "> ( ) observability"]
    );

    session.press("<Enter>");
    assert_eq!(section(&session, AREAS)[1], "> (x) observability");
}

/// The heading above the changes.
const CHANGES: &str = "Change to practice on";

/// Each row of a section split into its columns, which are at least two
/// cells apart.
fn columns(rows: &[String]) -> Vec<Vec<&str>> {
    rows.iter()
        .map(|row| {
            row.split("  ")
                .map(str::trim)
                .filter(|column| !column.is_empty())
                .collect()
        })
        .collect()
}

#[test]
fn the_change_list_shows_every_curated_change_with_its_message_and_areas() {
    let mut session = Session::plain(80, 24);

    session.press("<Enter>");

    assert_eq!(
        columns(&section(&session, CHANGES)),
        [
            ["( ) Model the payment lifecycle", "domain"],
            [
                "( ) Persist payments in an append-only file store",
                "infrastructure"
            ],
            [
                "( ) Call the payment provider with timeouts and retries",
                "infrastructure"
            ],
            ["( ) Serve payments over HTTP", "domain, infrastructure"],
            ["( ) Preserve payment retry identity", "domain"],
            [
                "( ) Put storage and the provider behind ports",
                "system design"
            ],
            [
                "( ) Trace requests and export payment metrics",
                "observability"
            ],
            [
                "( ) Cache payments per tenant",
                "scalability, infrastructure"
            ],
            ["( ) Rename ChargeOutcome to ProviderOutcome", "domain"],
            ["( ) Run rustfmt over the HTTP layer", "infrastructure"],
        ]
    );
}

#[test]
fn space_or_enter_on_a_change_chooses_it_and_only_it() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    // Past the five areas, onto the first change.
    session.press("j j j j j <Space>");
    let chosen = |session: &Session| -> Vec<String> {
        columns(&section(session, CHANGES))
            .into_iter()
            .map(|row| row[0].to_owned())
            .filter(|row| row.contains("(x)"))
            .collect()
    };
    assert_eq!(chosen(&session), ["> (x) Model the payment lifecycle"]);

    session.press("j j <Enter>");
    assert_eq!(
        chosen(&session),
        ["> (x) Call the payment provider with timeouts and retries"]
    );

    session.press("k");
    assert_eq!(
        chosen(&session),
        ["(x) Call the payment provider with timeouts and retries"],
        "moving on keeps the choice"
    );
}

#[test]
fn generate_questions_is_visible_but_disabled_until_jev_arrives() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");
    assert_eq!(
        session.row_with("Generate questions"),
        "Generate questions (arrives with Jev)"
    );

    // Shift+Tab wraps from the first control to the last.
    session.press("<S-Tab>");
    assert_eq!(
        session.row_with("Generate questions"),
        "> Generate questions (arrives with Jev)"
    );
    let picker = session.text();
    assert_eq!(session.press("<Enter> <Space>"), Flow::Continue);
    assert_eq!(session.text(), picker, "activating it does nothing");
}

#[test]
fn key_help_on_the_picker_says_space_toggles_an_area_and_q_goes_back() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter> ?");

    assert!(
        session.text().contains("Keys on Practice"),
        "{}",
        session.text()
    );
    let rows = session.overlay_rows();
    for (keys, does) in [
        ("j, Down, k, Up", "Move down or up"),
        ("Space, Enter", "Toggle an area, or choose a change"),
        ("q, Esc", "Back"),
        ("Tab, Shift+Tab", "Next or previous control"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.starts_with(keys) && row.ends_with(does)),
            "{does:?} is {keys:?}:\n{rows:#?}"
        );
    }
}

#[test]
fn the_palette_on_the_picker_offers_back_first_and_runs_it() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter> :");

    let rows = session.overlay_rows();
    assert!(
        session.text().contains("Actions on Practice"),
        "{}",
        session.text()
    );
    assert!(rows[0].starts_with("> Back"), "{rows:#?}");

    session.press("<Enter>");
    assert_eq!(
        session.row_with("Practice"),
        "> Practice on the showcase project",
        "back on home, with focus where it was"
    );
}

#[test]
fn the_footer_reminds_that_space_toggles_and_q_goes_back() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    assert_eq!(
        session.rows()[22].trim(),
        "? keys  : actions  Space toggle  q back"
    );
}

#[test]
fn q_and_esc_go_back_to_home_with_practice_still_focused() {
    for back in ["q", "<Esc>"] {
        let mut session = Session::plain(80, 24);
        session.press("<Enter> j <Space>");

        assert_eq!(session.press(back), Flow::Continue);
        assert_eq!(
            session.row_with("Practice"),
            "> Practice on the showcase project",
            "after {back}"
        );
    }
}

#[test]
fn the_picker_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press("<Enter>");

    insta::assert_debug_snapshot!("picker_120x32", session.frame());
}

#[test]
fn the_picker_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    insta::assert_debug_snapshot!("picker_80x24", session.frame());
}

/// From home: open the picker, deselect observability, choose the third
/// change, then move on to "Generate questions", the last control.
const CHOICES: &str = "<Enter> j <Space> j j j j j j <Space> j j j j j j j j";

#[test]
fn the_picker_without_colour_marks_focus_selection_and_disabled_with_text_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press(CHOICES);

    insta::assert_debug_snapshot!("picker_with_choices_120x32", session.frame());
}

#[test]
fn the_picker_without_colour_marks_focus_selection_and_disabled_with_text_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(CHOICES);

    insta::assert_debug_snapshot!("picker_with_choices_80x24", session.frame());
}

#[test]
fn the_picker_in_colour_at_80x24() {
    let mut session = Session::start(
        Setup {
            variables: &[("COLORTERM", "truecolor")],
            ..Setup::default()
        },
        80,
        24,
    );
    session.press(CHOICES);

    insta::assert_debug_snapshot!("picker_in_colour_80x24", session.frame());
}

#[test]
fn key_help_on_the_picker_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter> ?");

    insta::assert_debug_snapshot!("key_help_on_the_picker_80x24", session.frame());
}

/// From home: open the picker and move down to its last change.
const TO_THE_LAST_CHANGE: &str = "<Enter> j j j j j j j j j j j j j j";

#[test]
fn in_the_smallest_window_the_lists_scroll_to_keep_the_focused_row_in_view() {
    let mut session = Session::plain(50, 12);

    session.press(TO_THE_LAST_CHANGE);

    let text = session.text();
    assert!(
        text.contains("> ( ) Run rustfmt over the HTTP layer"),
        "{text}"
    );
    assert!(text.contains("Generate questions"), "{text}");
}

#[test]
fn in_the_smallest_window_the_chosen_change_stays_in_view_when_focus_moves_on() {
    let mut session = Session::plain(50, 12);
    session.press(TO_THE_LAST_CHANGE);

    session.press("<Space> j");

    let text = session.text();
    assert!(
        text.contains("(x) Run rustfmt over the HTTP layer"),
        "{text}"
    );
    assert!(text.contains("> Generate questions"), "{text}");
}
