//! The relevance gate on Preparing questions, against a scripted AI: before
//! any question is written, Jev decides, in one call, which chosen areas
//! the change is worth asking about. An unclear verdict lets an area
//! through and only a clear no drops it; when every chosen area is a clear
//! no, Nothing to practice in this change says so and offers Ask anyway.

use std::sync::Arc;

use whetstone_ai::{Asked, RequestFailure, ScriptedAi};
use whetstone_core::{CandidateCheck, Probability};

use crate::answering::submit;
use crate::preparing_questions::{
    DOMAIN, INFRASTRUCTURE, SERVE_OVER_HTTP, answer, area_id, candidate, generate, reasoning_met,
    stems,
};
use crate::session::Session;

/// The showcase area observability, third in the picker.
const OBSERVABILITY: usize = 2;

/// The requests `ai` was asked, as what each was for: the areas the gate
/// was asked about, the area written about, or the stem checked.
fn asked_for(ai: &ScriptedAi) -> Vec<String> {
    ai.asked()
        .iter()
        .map(|asked| match asked {
            Asked::CheckAreas { areas, .. } => {
                let ids: Vec<&str> = areas.iter().map(|area| area.id().as_str()).collect();
                format!("gate {}", ids.join(" "))
            }
            Asked::WriteCandidates { area, .. } => format!("write {}", area.id()),
            Asked::CheckQuestion { candidate, .. } => format!("check {}", candidate.stem()),
            Asked::AssessReasoning { stem, .. } => format!("assess {stem}"),
        })
        .collect()
}

/// The text of every row, trimmed, without the blank ones.
fn rows(session: &Session) -> Vec<String> {
    session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .filter(|row| !row.is_empty())
        .collect()
}

/// Jev's answer `value` in the relevance gate.
fn gate(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// How many candidates each write `ai` was asked for, by area.
fn written_counts(ai: &ScriptedAi) -> Vec<(String, usize)> {
    ai.asked()
        .iter()
        .filter_map(|asked| match asked {
            Asked::WriteCandidates { area, count, .. } => {
                Some((area.id().to_string(), count.get()))
            }
            Asked::CheckAreas { .. }
            | Asked::CheckQuestion { .. }
            | Asked::AssessReasoning { .. } => None,
        })
        .collect()
}

/// Scripts `ai` to write `count` candidates about the area at `place`,
/// named `name`, each passing every check, and its reasoning, once
/// submitted, meeting its criterion, for the change "Serve payments over
/// HTTP".
fn writing(ai: &ScriptedAi, place: usize, name: &str, count: usize) {
    let stems = stems(name, count);
    ai.on_write(
        &area_id(place),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", SERVE_OVER_HTTP))
            .collect()),
    );
    for stem in &stems {
        ai.on_check_passing(stem);
        reasoning_met(ai, stem);
    }
}

#[test]
fn before_any_question_is_written_jev_checks_every_chosen_area_in_one_call() {
    let ai = Arc::new(ScriptedAi::new());
    writing(&ai, DOMAIN, "Domain", 3);
    writing(&ai, INFRASTRUCTURE, "Infrastructure", 3);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);

    session.settle_once();

    assert_eq!(
        asked_for(&ai),
        ["gate domain infrastructure"],
        "the gate alone, before anything is written"
    );
    let text = session.text();
    for stage in [
        "✓ Preparing evidence",
        "⠋ Checking areas · Jev",
        "· Writing questions · generator",
    ] {
        assert!(text.contains(stage), "Checking areas is live: {text}");
    }

    session.settle_once();
    assert_eq!(
        asked_for(&ai),
        [
            "gate domain infrastructure",
            "write domain",
            "write infrastructure"
        ],
        "both areas passed, so both are written about"
    );
    let text = session.text();
    for stage in ["✓ Checking areas · Jev", "⠋ Writing questions · generator"] {
        assert!(text.contains(stage), "{text}");
    }
}

#[test]
fn a_clear_no_drops_its_area_and_only_the_areas_that_passed_are_written_about() {
    let ai = Arc::new(ScriptedAi::new());
    // At the lower cut-off, so clear no.
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.2));
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);

    session.settle();

    assert_eq!(
        written_counts(&ai),
        [("domain".to_owned(), 6)],
        "only the domain area is written about, for all 4 questions and 2 more"
    );
    assert_eq!(session.row_with("Question 1"), "Question 1 of 4 · Domain");
}

#[test]
fn an_unclear_verdict_lets_its_area_through() {
    let ai = Arc::new(ScriptedAi::new());
    // Just above the lower cut-off, so unclear.
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.21));
    writing(&ai, DOMAIN, "Domain", 3);
    writing(&ai, INFRASTRUCTURE, "Infrastructure", 3);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);

    session.settle();

    assert_eq!(
        written_counts(&ai),
        [("domain".to_owned(), 3), ("infrastructure".to_owned(), 3)],
        "both areas are written about, for (4 + 2) / 2 each"
    );
    submit(&mut session, "a", "Because.");
    session.press("<Enter>");
    assert_eq!(
        session.row_with("Question 2"),
        "Question 2 of 4 · Infrastructure"
    );
}

#[test]
fn below_the_first_question_one_line_names_the_chosen_areas_not_asked_about() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.05));
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);
    session.settle();

    let rows = rows(&session);
    let options = rows
        .iter()
        .position(|row| row.starts_with("( ) C  "))
        .expect("the first question's options show");
    assert_eq!(
        rows[options + 1],
        "Not asked about: Infrastructure (not worth asking about in this change)",
        "{rows:#?}"
    );

    submit(&mut session, "a", "Because.");
    session.press("<Enter>");
    assert!(
        !session.text().contains("Not asked about"),
        "only below the first question:\n{}",
        session.text()
    );
}

#[test]
fn when_every_chosen_area_passes_no_line_names_areas_not_asked_about() {
    let ai = Arc::new(ScriptedAi::new());
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN], SERVE_OVER_HTTP, 120, 32);

    session.settle();

    assert!(session.text().contains("Question 1 of 4"));
    assert!(
        !session.text().contains("Not asked about"),
        "{}",
        session.text()
    );
}

/// A session `width` by `height` in which Jev's relevance gate clearly says
/// neither chosen area, domain and infrastructure, is worth asking about in
/// the change "Serve payments over HTTP". Nothing has been written.
fn nothing_to_practice(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    ai.on_gate(&area_id(DOMAIN), gate(0.1));
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.2));
    let mut session = generate(
        ai,
        &[DOMAIN, INFRASTRUCTURE],
        SERVE_OVER_HTTP,
        width,
        height,
    );
    session.settle();
    session
}

#[test]
fn when_every_chosen_area_is_a_clear_no_nothing_to_practice_gives_each_its_verdict() {
    let ai = Arc::new(ScriptedAi::new());

    let session = nothing_to_practice(&ai, 120, 32);

    assert_eq!(
        asked_for(&ai),
        ["gate domain infrastructure"],
        "nothing is written"
    );
    let rows = rows(&session);
    for row in [
        "Nothing to practice in this change",
        "Domain: not worth asking about in this change",
        "Infrastructure: not worth asking about in this change",
        "> Pick another change",
        "Pick other areas",
        "Ask anyway",
    ] {
        assert!(rows.contains(&row.to_owned()), "{row}: {rows:#?}");
    }
    assert!(
        rows.iter()
            .any(|row| row.contains("✓ Checking areas · Jev")),
        "{rows:#?}"
    );
}

#[test]
fn nothing_to_practice_at_120x32() {
    let ai = Arc::new(ScriptedAi::new());
    let session = nothing_to_practice(&ai, 120, 32);

    insta::assert_debug_snapshot!("nothing_to_practice_120x32", session.frame());
}

#[test]
fn nothing_to_practice_at_80x24() {
    let ai = Arc::new(ScriptedAi::new());
    let session = nothing_to_practice(&ai, 80, 24);

    insta::assert_debug_snapshot!("nothing_to_practice_80x24", session.frame());
}

#[test]
fn pick_another_change_and_pick_other_areas_go_back_to_the_areas_and_changes() {
    for keys in ["<Enter>", "j <Enter>", "q"] {
        let ai = Arc::new(ScriptedAi::new());
        let mut session = nothing_to_practice(&ai, 80, 24);

        session.press(keys);

        assert!(
            session.text().contains("Change to practice on"),
            "{keys}: {}",
            session.text()
        );
        assert_eq!(ai.asked().len(), 1, "{keys}: nothing more is asked");
    }
}

#[test]
fn ask_anyway_writes_about_every_chosen_area_without_the_gate_and_still_checks_each_question() {
    let ai = Arc::new(ScriptedAi::new());
    let mut session = nothing_to_practice(&ai, 120, 32);
    writing(&ai, DOMAIN, "Domain", 3);
    writing(&ai, INFRASTRUCTURE, "Infrastructure", 3);

    session.press("j j <Enter>");
    let preparing = session.text();
    session.settle();

    assert!(
        preparing.contains("Preparing questions"),
        "preparation runs again: {preparing}"
    );
    assert!(
        preparing.contains("– Checking areas · Jev"),
        "without the gate: {preparing}"
    );
    let asked = asked_for(&ai);
    assert_eq!(
        asked[..3],
        [
            "gate domain infrastructure",
            "write domain",
            "write infrastructure"
        ],
        "the gate isn't asked again, and both areas are written about"
    );
    assert_eq!(
        written_counts(&ai),
        [("domain".to_owned(), 3), ("infrastructure".to_owned(), 3)]
    );
    assert_eq!(
        asked
            .iter()
            .filter(|asked| asked.starts_with("check "))
            .count(),
        6,
        "every candidate is checked: {asked:#?}"
    );
    let text = session.text();
    assert!(text.contains("Question 1 of 4 · Domain"), "{text}");
    assert!(
        !text.contains("Not asked about"),
        "every chosen area was asked about: {text}"
    );
}

/// Scripts `ai` to write `count` candidates about the area at `place`,
/// named `name`, for the change "Serve payments over HTTP", none of which
/// passes its question check: Jev can't tell whether each needs the change.
fn writing_none_passing(ai: &ScriptedAi, place: usize, name: &str, count: usize) {
    let stems = stems(&format!("{name} failing"), count);
    ai.on_write(
        &area_id(place),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", SERVE_OVER_HTTP))
            .collect()),
    );
    for stem in &stems {
        ai.on_check_passing_except(stem, [answer(CandidateCheck::NeedsChange, 0.5)]);
    }
}

#[test]
fn retry_when_no_question_passed_keeps_the_gates_result() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.1));
    writing_none_passing(&ai, DOMAIN, "Domain", 6);
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);
    session.settle();
    assert!(
        session.text().contains("No questions passed checks"),
        "{}",
        session.text()
    );
    let before = ai.asked().len();

    session.press("<Enter>");
    session.settle();

    let again = &asked_for(&ai)[before..];
    assert_eq!(
        again.first().map(String::as_str),
        Some("write domain"),
        "the gate isn't asked again: {again:#?}"
    );
    assert!(
        !again.contains(&"write infrastructure".to_owned()),
        "and the area it dropped stays dropped: {again:#?}"
    );
    assert_eq!(
        session.row_with("Not asked about"),
        "Not asked about: Infrastructure (not worth asking about in this change)"
    );
}

#[test]
fn retry_when_no_question_passed_after_ask_anyway_asks_about_every_chosen_area_again() {
    let ai = Arc::new(ScriptedAi::new());
    let mut session = nothing_to_practice(&ai, 120, 32);
    writing_none_passing(&ai, DOMAIN, "Domain", 3);
    writing_none_passing(&ai, INFRASTRUCTURE, "Infrastructure", 3);
    writing(&ai, DOMAIN, "Domain", 3);
    writing(&ai, INFRASTRUCTURE, "Infrastructure", 3);
    session.press("j j <Enter>");
    session.settle();
    assert!(
        session.text().contains("No questions passed checks"),
        "{}",
        session.text()
    );
    let before = ai.asked().len();

    session.press("<Enter>");
    session.settle();

    assert_eq!(
        asked_for(&ai)[before..before + 2],
        ["write domain", "write infrastructure"],
        "no gate, and both areas again"
    );
    assert!(session.text().contains("Question 1 of 4"));
}

#[test]
fn a_failed_gate_names_its_stage_and_retry_asks_it_again_and_nothing_else() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_gate_failing(RequestFailure::RateLimited);
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.1));
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 80, 24);
    session.settle();

    assert_eq!(
        asked_for(&ai),
        ["gate domain infrastructure"],
        "asked once, and never again until Retry"
    );
    let rows = rows(&session);
    for row in ["Checking areas failed: rate limited.", "> Retry", "Back"] {
        assert!(rows.contains(&row.to_owned()), "{row}: {rows:#?}");
    }
    assert!(
        rows.iter()
            .any(|row| row.contains("✗ Checking areas · Jev")),
        "{rows:#?}"
    );

    session.press("<Enter>");
    session.settle();

    assert_eq!(
        asked_for(&ai)[..3],
        [
            "gate domain infrastructure",
            "gate domain infrastructure",
            "write domain"
        ],
        "Retry asks the gate again, and its answer decides what is written"
    );
    assert_eq!(session.row_with("Question 1"), "Question 1 of 4 · Domain");
}

#[test]
fn back_after_a_failed_gate_returns_to_the_areas_and_changes() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_gate_failing(RequestFailure::TimedOut);
    let mut session = generate(&ai, &[DOMAIN], SERVE_OVER_HTTP, 80, 24);
    session.settle();
    assert!(
        session.text().contains("Checking areas failed: timed out."),
        "{}",
        session.text()
    );

    session.press("j <Enter>");

    assert!(session.text().contains("Change to practice on"));
    assert_eq!(ai.asked().len(), 1);
}

#[test]
fn cancel_while_jev_checks_the_areas_returns_to_the_areas_and_changes_and_its_late_answer_changes_nothing()
 {
    let ai = Arc::new(ScriptedAi::new());
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN], SERVE_OVER_HTTP, 80, 24);
    session.settle_once();
    assert_eq!(asked_for(&ai), ["gate domain"]);

    session.press("<Enter>");
    let picker = session.text();
    assert!(picker.contains("Change to practice on"), "{picker}");
    session.settle();

    assert_eq!(session.text(), picker, "the late answer changes nothing");
    assert_eq!(asked_for(&ai), ["gate domain"], "and nothing is written");
}

#[test]
fn jev_judges_each_area_by_its_name_and_description_and_no_area_prompt_reaches_it() {
    let ai = Arc::new(ScriptedAi::new());
    writing(&ai, DOMAIN, "Domain", 3);
    writing(&ai, INFRASTRUCTURE, "Infrastructure", 3);
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);

    session.settle();

    let asked = ai.asked();
    let [
        Asked::CheckAreas {
            evidence: judged,
            areas,
            sent,
        },
        Asked::WriteCandidates {
            evidence: written_from,
            ..
        },
        ..,
    ] = asked.as_slice()
    else {
        panic!("the gate, then the writes: {asked:#?}");
    };
    let showcase = whetstone::embedded_showcase();
    let chosen = [
        showcase.areas()[DOMAIN].clone(),
        showcase.areas()[INFRASTRUCTURE].clone(),
    ];
    assert_eq!(areas, &chosen, "the chosen areas, in order");
    assert_eq!(
        judged, written_from,
        "the gate judges the evidence the questions are written from"
    );
    let sent = std::iter::once(sent.state.to_string())
        .chain(
            sent.questions
                .values()
                .map(|question| format!("{question:?}")),
        )
        .collect::<String>();
    for area in &chosen {
        assert!(sent.contains(area.name()), "{}", area.id());
        assert!(sent.contains(area.description()), "{}", area.id());
        for line in area.prompt().lines().map(str::trim) {
            assert!(
                line.is_empty() || !sent.contains(line),
                "{}'s area prompt reached Jev: {line:?}",
                area.id()
            );
        }
    }
}

/// A session `width` by `height` in which Jev checks whether the domain
/// area is worth asking about in the change "Serve payments over HTTP".
fn checking_areas(width: u16, height: u16) -> Session {
    let ai = Arc::new(ScriptedAi::new());
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(&ai, &[DOMAIN], SERVE_OVER_HTTP, width, height);
    session.settle_once();
    session
}

#[test]
fn preparing_questions_while_jev_checks_the_areas_at_120x32() {
    insta::assert_debug_snapshot!(
        "preparing_questions_checking_areas_120x32",
        checking_areas(120, 32).frame()
    );
}

#[test]
fn preparing_questions_while_jev_checks_the_areas_at_80x24() {
    insta::assert_debug_snapshot!(
        "preparing_questions_checking_areas_80x24",
        checking_areas(80, 24).frame()
    );
}

/// A session `width` by `height` on the first question, whose areas are
/// domain, infrastructure and observability, the last two of which Jev's
/// relevance gate clearly said aren't worth asking about.
fn two_areas_not_asked_about(width: u16, height: u16) -> Session {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_gate(&area_id(INFRASTRUCTURE), gate(0.1));
    ai.on_gate(&area_id(OBSERVABILITY), gate(0.1));
    writing(&ai, DOMAIN, "Domain", 6);
    let mut session = generate(
        &ai,
        &[DOMAIN, INFRASTRUCTURE, OBSERVABILITY],
        SERVE_OVER_HTTP,
        width,
        height,
    );
    session.settle();
    session
}

#[test]
fn the_areas_not_asked_about_below_the_first_question_at_120x32() {
    insta::assert_debug_snapshot!(
        "areas_not_asked_about_120x32",
        two_areas_not_asked_about(120, 32).frame()
    );
}

#[test]
fn the_areas_not_asked_about_below_the_first_question_at_80x24() {
    insta::assert_debug_snapshot!(
        "areas_not_asked_about_80x24",
        two_areas_not_asked_about(80, 24).frame()
    );
}
