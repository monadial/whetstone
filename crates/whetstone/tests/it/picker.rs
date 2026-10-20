//! The area and change picker: "Practice on the showcase project" opens it.
//! The learner chooses areas, all selected at first, and one change from the
//! showcase project's curated history. "Generate questions" waits for Jev.

use whetstone::Flow;

use crate::session::{Session, Setup, columns};

#[test]
fn practice_on_the_showcase_project_opens_the_picker_from_home() {
    let mut session = Session::plain(80, 24);

    session.press("<Enter>");

    assert_eq!(session.rows()[1].trim(), "Practice on the showcase project");
}

/// The heading above the areas.
const AREAS: &str = "Areas the questions focus on";

#[test]
fn the_showcase_project_offers_the_five_default_areas_in_file_name_order_all_selected() {
    let mut session = Session::plain(80, 24);

    session.press("<Enter>");

    assert_eq!(
        columns(&session.section(AREAS))
            .into_iter()
            .map(|row| row[0])
            .collect::<Vec<_>>(),
        [
            "> (x) Domain",
            "(x) Infrastructure",
            "(x) Observability",
            "(x) Scalability",
            "(x) System design",
        ]
    );
}

#[test]
fn each_area_shows_its_one_line_description_in_a_column_of_its_own() {
    let mut session = Session::plain(120, 32);

    session.press("<Enter>");

    let descriptions = [
        "The rules, concepts and language the work is about",
        "Where the work runs and what it depends on",
        "How you can tell what the work is doing and why",
        "How the work holds up as use and data grow",
        "How the parts fit together, and why this way",
    ];
    assert_eq!(
        columns(&session.section(AREAS))
            .into_iter()
            .map(|row| row[1..].join("  "))
            .collect::<Vec<_>>(),
        descriptions
    );
    let starts: Vec<usize> = descriptions
        .iter()
        .map(|description| {
            session
                .rows()
                .iter()
                .find_map(|row| row.find(description))
                .expect("the description is shown")
        })
        .collect();
    assert!(
        starts.iter().all(|&start| start == starts[0]),
        "the descriptions start in one column: {starts:?}\n{}",
        session.text()
    );
}

#[test]
fn space_and_enter_toggle_the_focused_area() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    let area =
        |session: &Session, index: usize| columns(&session.section(AREAS))[index][0].to_owned();
    session.press("<Space>");
    assert_eq!(area(&session, 0), "> ( ) Domain");

    session.press("j <Space>");
    assert_eq!(
        [area(&session, 0), area(&session, 1)],
        ["( ) Domain", "> ( ) Infrastructure"]
    );

    session.press("<Enter>");
    assert_eq!(area(&session, 1), "> (x) Infrastructure");
}

/// The heading above the changes.
const CHANGES: &str = "Change to practice on";

#[test]
fn the_change_list_shows_every_curated_change_with_its_message_and_areas() {
    let mut session = Session::plain(80, 24);

    session.press("<Enter>");

    assert_eq!(
        columns(&session.section(CHANGES)),
        [
            ["( ) Model the payment lifecycle", "Domain"],
            [
                "( ) Persist payments in an append-only file store",
                "Infrastructure"
            ],
            [
                "( ) Call the payment provider with timeouts and retries",
                "Infrastructure"
            ],
            ["( ) Serve payments over HTTP", "Domain, Infrastructure"],
            ["( ) Preserve payment retry identity", "Domain"],
            [
                "( ) Put storage and the provider behind ports",
                "System design"
            ],
            [
                "( ) Trace requests and export payment metrics",
                "Observability"
            ],
            [
                "( ) Cache payments per tenant",
                "Scalability, Infrastructure"
            ],
            ["( ) Rename ChargeOutcome to ProviderOutcome", "Domain"],
            ["( ) Run rustfmt over the HTTP layer", "Infrastructure"],
        ]
    );
}

#[test]
fn space_on_a_change_chooses_it_and_only_it() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    // Past the five areas, onto the first change.
    session.press("j j j j j <Space>");
    let chosen = |session: &Session| -> Vec<String> {
        columns(&session.section(CHANGES))
            .into_iter()
            .map(|row| row[0].to_owned())
            .filter(|row| row.contains("(x)"))
            .collect()
    };
    assert_eq!(chosen(&session), ["> (x) Model the payment lifecycle"]);

    session.press("j j <Space>");
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
fn generate_questions_is_visible_but_disabled_without_an_openrouter_key() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");
    assert_eq!(
        session.row_with("Generate questions"),
        "Generate questions (needs OPENROUTER_API_KEY)"
    );

    // Shift+Tab wraps from the first control to the last.
    session.press("<S-Tab>");
    assert_eq!(
        session.row_with("Generate questions"),
        "> Generate questions (needs OPENROUTER_API_KEY)"
    );
    let picker = session.text();
    assert_eq!(session.press("<Enter> <Space>"), Flow::Continue);
    assert_eq!(session.text(), picker, "activating it does nothing");
}

#[test]
fn key_help_on_the_picker_says_space_toggles_an_area_enter_opens_a_change_and_q_goes_back() {
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
        ("Space", "Toggle an area, or choose a change"),
        ("Enter", "Toggle an area, or choose and open a change"),
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
fn the_footer_reminds_that_space_toggles_enter_opens_and_q_goes_back() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter>");

    assert_eq!(
        session.rows()[22].trim(),
        "? keys  : actions  Space toggle  Enter open  q back"
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

/// From home: open the picker, deselect infrastructure, choose the third
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

#[test]
fn the_palette_opens_the_picker_from_any_screen() {
    let mut session = Session::plain(80, 24);
    session.press(": j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Practice on the showcase project");

    // From Settings, past its own Back and then Home.
    let mut session = Session::plain(80, 24);
    session.press("j <Enter> : j j");
    assert!(
        session
            .overlay_rows()
            .iter()
            .any(|row| row.starts_with("> Practice") && row.ends_with("Choose areas and a change")),
        "{:#?}",
        session.overlay_rows()
    );
    session.press("<Enter>");
    assert_eq!(session.rows()[1].trim(), "Practice on the showcase project");
}

#[test]
fn with_no_area_selected_the_picker_asks_for_at_least_one() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter> <Space> j <Space> j <Space> j <Space> j <Space>");

    assert_eq!(
        session.row_with(AREAS),
        "Areas the questions focus on: choose at least one"
    );

    session.press("<Space>");
    assert_eq!(session.row_with(AREAS), AREAS);
}
