//! Preparing questions on the showcase project, against a scripted AI:
//! Generate questions finds the change's connected code, then Jev's
//! relevance gate lets the chosen areas through (`relevance_gate.rs` tests
//! the gate), one generator call per area writes candidates, plain checks
//! drop malformed ones, Jev checks each one left, the show rule keeps the
//! clear-cut ones and picking chooses up to `questions` of them.

use std::sync::Arc;
use std::time::Duration;

use whetstone_ai::{Asked, QuestionCheckCall, RequestFailure, ScriptedAi};
use whetstone_core::{
    AreaId, CandidateCheck, ConnectionsState, Evidence, OptionLetter, Probability, QuestionType,
    WrittenCandidate, WrittenCitation, WrittenCriterion, WrittenOption,
};

use crate::answering::submit;
use crate::session::{NO_COLOR, Session, Setup};

/// The showcase project's areas, in the order the picker offers them.
pub(crate) const DOMAIN: usize = 0;
pub(crate) const INFRASTRUCTURE: usize = 1;

/// The showcase change "Model the payment lifecycle".
pub(crate) const PAYMENT_LIFECYCLE: usize = 0;
/// The showcase change "Serve payments over HTTP".
pub(crate) const SERVE_OVER_HTTP: usize = 3;

/// How many curated changes the showcase project has.
const CHANGES: usize = 10;
/// How many areas the showcase project offers.
const AREAS: usize = 5;

/// The ID of the showcase area at `place` in the picker.
pub(crate) fn area_id(place: usize) -> AreaId {
    whetstone::embedded_showcase().areas()[place].id().clone()
}

/// A session `width` by `height`, without colour, whose Jev and generator
/// are `ai`, on the showcase picker with only the areas at `areas` selected
/// and the change at `change` chosen, after activating Generate questions.
/// Nothing the app asked has been delivered yet.
pub(crate) fn generate(
    ai: &Arc<ScriptedAi>,
    areas: &[usize],
    change: usize,
    width: u16,
    height: u16,
) -> Session {
    generate_in(
        Setup {
            variables: NO_COLOR,
            ai: Some(ai),
            ..Setup::default()
        },
        areas,
        change,
        width,
        height,
    )
}

/// The same as [`generate`], in a session started as `setup` says.
pub(crate) fn generate_in(
    setup: Setup<'_>,
    areas: &[usize],
    change: usize,
    width: u16,
    height: u16,
) -> Session {
    let mut session = Session::start(setup, width, height);
    session.press("<Enter>");
    for place in 0..AREAS {
        if !areas.contains(&place) {
            session.press("<Space>");
        }
        session.press("j");
    }
    session.press(&"j ".repeat(change));
    session.press("<Space>");
    session.press(&"j ".repeat(CHANGES - change));
    session.press("<Enter>");
    session
}

/// The path of the first file the showcase change at `change` touches,
/// which its evidence sends whole.
pub(crate) fn touched_path(change: usize) -> String {
    whetstone::embedded_showcase().changes()[change].files()[0]
        .path()
        .to_owned()
}

/// A well-formed single-choice candidate asking `stem`, whose answer key is
/// `answer_key`, citing the first line of the first file the change at
/// `change` touches.
pub(crate) fn candidate(stem: &str, answer_key: &str, change: usize) -> WrittenCandidate {
    let text = str::to_owned;
    WrittenCandidate {
        question_type: QuestionType::SingleChoice,
        stem: text(stem),
        options: [
            ("A", "Option A of the question"),
            ("B", "Option B of the question"),
            ("C", "Option C of the question"),
        ]
        .map(|(letter, option)| WrittenOption {
            letter: text(letter),
            text: text(option),
            rationale: format!("The rationale for {letter}, which stays hidden"),
        })
        .into(),
        answer_key: text(answer_key),
        why_it_matters: text("Why it matters, which stays hidden"),
        rubric: vec![WrittenCriterion {
            statement: text("The rubric's criterion, which stays hidden"),
            met: text("What meeting it looks like"),
            not_met: text("What not meeting it looks like"),
        }],
        hint: text("The hint, which stays hidden"),
        citations: vec![WrittenCitation {
            path: touched_path(change),
            first_line: 1,
            last_line: 1,
        }],
    }
}

/// Jev's answer `value` to the question check's `check`.
pub(crate) fn answer(check: CandidateCheck, value: f64) -> (CandidateCheck, Probability) {
    (check, Probability::new(value).expect("from 0 to 1"))
}

/// Jev's option ratings in a single-choice question check: the probability
/// that each of A, B and C correctly answers the question.
pub(crate) fn ratings(a: f64, b: f64, c: f64) -> [(CandidateCheck, Probability); 3] {
    [
        (OptionLetter::A, a),
        (OptionLetter::B, b),
        (OptionLetter::C, c),
    ]
    .map(|(letter, value)| answer(CandidateCheck::Option(letter), value))
}

/// The stems of `count` candidates about `area`, numbered from 1.
pub(crate) fn stems(area: &str, count: usize) -> Vec<String> {
    (1..=count)
        .map(|number| format!("{area} question {number}?"))
        .collect()
}

#[test]
fn home_says_plainly_what_practice_on_the_showcase_project_sends_and_who_pays() {
    let session = Session::plain(120, 32);

    assert!(
        session.text().contains(
            "Practice sends showcase excerpts and your answers to OpenRouter, paid with your \
             credits."
        ),
        "{}",
        session.text()
    );
}

#[test]
fn generate_questions_writes_checks_and_shows_the_first_of_the_questions_that_pass() {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 6);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    for stem in &stems {
        ai.on_check_passing(stem);
    }

    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    assert!(
        session.text().contains("Preparing questions"),
        "{}",
        session.text()
    );
    assert!(
        ai.asked().is_empty(),
        "nothing is asked before connected code is found"
    );
    session.settle();

    let asked = ai.asked();
    let [
        Asked::CheckAreas { .. },
        Asked::WriteCandidates { area, count, .. },
        checks @ ..,
    ] = asked.as_slice()
    else {
        panic!("the relevance gate, one write, then the checks: {asked:#?}");
    };
    assert_eq!(area.id(), &area_id(DOMAIN));
    assert_eq!(count.get(), 6, "4 questions and 2 more, for one area");
    assert_eq!(checks.len(), 24, "a check of four calls per candidate");
    let text = session.text();
    assert!(text.contains("Question 1 of 4"), "{text}");
    assert!(text.contains("Domain"), "{text}");
    assert!(text.contains("Domain question 1?"), "{text}");
    for option in ["Option A", "Option B", "Option C"] {
        assert!(text.contains(option), "{option}: {text}");
    }
    assert!(
        !text.contains("hidden"),
        "the answer key's parts stay hidden: {text}"
    );
}

#[test]
fn one_generator_call_per_chosen_area_goes_out_at_once_then_one_check_per_candidate() {
    let ai = Arc::new(ScriptedAi::new());
    let domain = stems("Domain", 3);
    let infrastructure = stems("Infrastructure", 3);
    for (area, stems) in [(DOMAIN, &domain), (INFRASTRUCTURE, &infrastructure)] {
        ai.on_write(
            &area_id(area),
            Ok(stems
                .iter()
                .map(|stem| candidate(stem, "A", SERVE_OVER_HTTP))
                .collect()),
        );
        for stem in stems {
            ai.on_check_passing(stem);
            reasoning_met(&ai, stem);
        }
    }
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);
    // The relevance gate first.
    session.settle_once();

    session.settle_once();
    let writes: Vec<(String, usize)> = ai
        .asked()
        .iter()
        .filter_map(|asked| match asked {
            Asked::CheckAreas { .. } => None,
            Asked::WriteCandidates { area, count, .. } => {
                Some((area.id().to_string(), count.get()))
            }
            Asked::CheckQuestion { .. }
            | Asked::AssessReasoning { .. }
            | Asked::Explain { .. }
            | Asked::CheckExplanation { .. } => {
                panic!("a check before every area is written")
            }
        })
        .collect();
    assert_eq!(
        writes,
        [("domain".to_owned(), 3), ("infrastructure".to_owned(), 3)],
        "both areas asked before either replies, each for (4 + 2) / 2"
    );

    session.settle_once();
    let checked: Vec<String> = ai
        .asked()
        .iter()
        .filter_map(|asked| match asked {
            Asked::CheckQuestion { candidate, .. } => Some(candidate.stem().to_owned()),
            Asked::CheckAreas { .. }
            | Asked::WriteCandidates { .. }
            | Asked::AssessReasoning { .. }
            | Asked::Explain { .. }
            | Asked::CheckExplanation { .. } => None,
        })
        .collect();
    assert_eq!(
        checked,
        [domain.clone(), infrastructure.clone()]
            .concat()
            .into_iter()
            .flat_map(|stem| [stem.clone(), stem.clone(), stem.clone(), stem])
            .collect::<Vec<_>>(),
        "each call of every candidate's check sent before any replies, in the order written"
    );
    assert!(session.text().contains("Preparing questions"));

    session.settle_once();
    let mut areas = Vec::new();
    for _ in 0..4 {
        areas.push(session.question().row_with("Question "));
        submit(&mut session, "a", "Because.");
        session.press("<Enter>");
    }
    assert_eq!(
        areas,
        [
            "Question 1 of 4 · Domain",
            "Question 2 of 4 · Infrastructure",
            "Question 3 of 4 · Domain",
            "Question 4 of 4 · Infrastructure",
        ],
        "picking alternates across the areas"
    );
}

/// A scripted AI that writes `count` candidates about the domain area, each
/// passing every check, and its reasoning, once submitted, meeting its
/// criterion, for the change at `change`.
pub(crate) fn writing_about_the_domain(count: usize, change: usize) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    write_about_the_domain(&ai, count, change);
    ai
}

/// Scripts `ai` to write `count` candidates about the domain area once
/// more, each passing every check, and its reasoning, once submitted,
/// meeting its criterion, for the change at `change`.
fn write_about_the_domain(ai: &ScriptedAi, count: usize, change: usize) {
    let stems = stems("Domain", count);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", change))
            .collect()),
    );
    for stem in &stems {
        ai.on_check_passing(stem);
        reasoning_met(ai, stem);
    }
}

/// Scripts Jev to find the reasoning written for the question `stem`,
/// whose one criterion is [`candidate`]'s, meeting it, once submitted.
pub(crate) fn reasoning_met(ai: &ScriptedAi, stem: &str) {
    ai.on_assess(stem, Ok(vec![Probability::new(0.9).expect("from 0 to 1")]));
}

#[test]
fn preparing_questions_while_connected_code_is_found_at_120x32() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);

    insta::assert_debug_snapshot!("preparing_questions_120x32", session.frame());
}

#[test]
fn preparing_questions_while_connected_code_is_found_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);

    insta::assert_debug_snapshot!("preparing_questions_80x24", session.frame());
}

#[test]
fn preparing_questions_while_the_generator_writes_at_120x32() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    session.settle_once();
    session.settle_once();

    insta::assert_debug_snapshot!("preparing_questions_writing_120x32", session.frame());
}

#[test]
fn preparing_questions_while_jev_checks_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
    session.settle_once();
    session.settle_once();
    session.settle_once();

    insta::assert_debug_snapshot!("preparing_questions_checking_80x24", session.frame());
}

/// A session whose generator fails to write about the domain area, rate
/// limited.
fn writing_rate_limited(width: u16, height: u16) -> Session {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(&area_id(DOMAIN), Err(RequestFailure::RateLimited));
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, width, height);
    session.settle();
    session
}

#[test]
fn a_failed_request_at_120x32() {
    insta::assert_debug_snapshot!(
        "preparing_questions_failed_120x32",
        writing_rate_limited(120, 32).frame()
    );
}

#[test]
fn a_failed_request_at_80x24() {
    insta::assert_debug_snapshot!(
        "preparing_questions_failed_80x24",
        writing_rate_limited(80, 24).frame()
    );
}

/// The requests `ai` was asked, as what each was for: the area written
/// about, the stem checked with the call, or the stem whose reasoning was
/// assessed or explained, or whose explanation was checked.
fn asked_for(ai: &ScriptedAi) -> Vec<String> {
    ai.asked()
        .iter()
        .map(|asked| match asked {
            Asked::CheckAreas { .. } => "gate".to_owned(),
            Asked::WriteCandidates { area, .. } => format!("write {}", area.id()),
            Asked::CheckQuestion {
                call, candidate, ..
            } => format!("check {} ({call:?})", candidate.stem()),
            Asked::AssessReasoning { stem, .. } => format!("assess {stem}"),
            Asked::Explain { stem, .. } => format!("explain {stem}"),
            Asked::CheckExplanation { stem, .. } => format!("check the explanation of {stem}"),
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

#[test]
fn the_showcase_changes_connected_code_reaches_the_evidence_the_generator_and_jev_are_given() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![candidate("Served?", "A", SERVE_OVER_HTTP)]),
    );
    ai.on_check_passing("Served?");
    let mut session = generate(&ai, &[DOMAIN], SERVE_OVER_HTTP, 120, 32);

    session.settle();

    let asked = ai.asked();
    let [
        Asked::CheckAreas { .. },
        Asked::WriteCandidates {
            evidence: written_from,
            ..
        },
        checks @ ..,
    ] = asked.as_slice()
    else {
        panic!("the relevance gate, one write and one check: {asked:#?}");
    };
    let checked_against: Vec<&Evidence> = checks
        .iter()
        .filter_map(|asked| match asked {
            Asked::CheckQuestion { evidence, .. } => Some(evidence),
            _ => None,
        })
        .collect();
    assert_eq!(
        checked_against.len(),
        4,
        "one check's four calls: {asked:#?}"
    );
    assert_eq!(written_from.connections_state(), ConnectionsState::Found);
    assert_ne!(
        written_from.connections().len(),
        0,
        "connected code is sent"
    );
    assert_eq!(
        written_from.areas(),
        &[whetstone::embedded_showcase().areas()[DOMAIN].clone()],
        "only the chosen area"
    );
    assert!(
        written_from
            .message()
            .starts_with("Serve payments over HTTP"),
        "the change's own message"
    );
    assert_eq!(
        checked_against, [written_from; 4],
        "Jev checks against the same evidence"
    );
    assert!(session.text().contains("Served?"));
}

#[test]
fn when_finding_connected_code_takes_too_long_questions_are_written_without_it() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![candidate(
            "Without connected code?",
            "A",
            SERVE_OVER_HTTP,
        )]),
    );
    ai.on_check_passing("Without connected code?");
    let mut session = generate_in(
        Setup {
            variables: NO_COLOR,
            ai: Some(&ai),
            time_limit: Some(Duration::ZERO),
            ..Setup::default()
        },
        &[DOMAIN],
        SERVE_OVER_HTTP,
        120,
        32,
    );

    session.wait(Duration::ZERO);

    assert!(
        session
            .text()
            .contains("– Finding connected code → ✓ Preparing evidence"),
        "{}",
        session.text()
    );
    let asked = ai.asked();
    let [Asked::CheckAreas { evidence, .. }] = asked.as_slice() else {
        panic!("Jev checks the areas at once: {asked:#?}");
    };
    assert_eq!(evidence.connections_state(), ConnectionsState::TookTooLong);
    assert_eq!(evidence.connections().len(), 0, "no connected code is sent");

    session.settle();
    assert!(
        session.text().contains("Without connected code?"),
        "the analysis's late result changes nothing:\n{}",
        session.text()
    );
    assert_eq!(ai.asked().len(), 6, "nothing is asked again");
}

#[test]
fn cancel_returns_to_the_areas_and_changes_and_a_request_that_finishes_after_it_changes_nothing() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
    session.settle_once();
    session.settle_once();
    assert_eq!(asked_for(&ai), ["gate", "write domain"]);

    session.press("<Enter>");
    let picker = session.text();
    assert!(picker.contains("Change to practice on"), "{picker}");
    session.settle();

    assert_eq!(session.text(), picker, "the late write changes nothing");
    assert_eq!(
        asked_for(&ai),
        ["gate", "write domain"],
        "and starts no check"
    );
}

#[test]
fn q_cancels_while_jev_checks_and_no_late_answer_changes_anything() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
    session.settle_once();
    session.settle_once();
    session.settle_once();
    let asked = ai.asked().len();

    session.press("q");
    let picker = session.text();
    session.settle();

    assert!(picker.contains("Change to practice on"), "{picker}");
    assert_eq!(session.text(), picker);
    assert_eq!(ai.asked().len(), asked);
}

#[test]
fn cancel_while_connected_code_is_found_asks_nothing_once_it_is_found() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);

    session.press("<Esc>");
    let picker = session.text();
    session.settle();

    assert_eq!(session.text(), picker);
    assert!(ai.asked().is_empty(), "{:#?}", ai.asked());
}

#[test]
fn going_home_while_preparing_cancels_too() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
    session.settle_once();
    session.settle_once();

    // The palette's first action after Cancel is Home.
    session.press(": j <Enter>");
    let home = session.text();
    assert!(home.contains("Practice on the showcase project"), "{home}");
    session.settle();

    assert_eq!(session.text(), home);
    assert_eq!(asked_for(&ai), ["gate", "write domain"]);
}

#[test]
fn generate_questions_again_after_cancel_prepares_afresh() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    write_about_the_domain(&ai, 6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
    session.settle_once();
    session.settle_once();
    session.press("q");

    session.press("<Enter>");
    session.settle();

    assert_eq!(
        session.question().row_with("Question 1"),
        "Question 1 of 4 · Domain"
    );
    assert_eq!(
        asked_for(&ai)
            .iter()
            .filter(|asked| asked.starts_with("write"))
            .count(),
        2
    );
}

#[test]
fn a_failed_request_names_its_stage_and_offers_retry_and_back_and_nothing_is_retried_automatically()
{
    let mut session = {
        let ai = Arc::new(ScriptedAi::new());
        ai.on_write(&area_id(DOMAIN), Err(RequestFailure::RateLimited));
        let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
        session.settle();
        assert_eq!(
            asked_for(&ai),
            ["gate", "write domain"],
            "asked once, and never again"
        );
        session
    };

    let rows = rows(&session);
    assert!(
        rows.contains(&"Writing questions failed: rate limited.".to_owned()),
        "{rows:#?}"
    );
    assert!(rows.contains(&"> Retry".to_owned()), "{rows:#?}");
    assert!(rows.contains(&"Back".to_owned()), "{rows:#?}");
    assert!(
        rows.iter()
            .any(|row| row.contains("✗ Writing questions · generator")),
        "{rows:#?}"
    );

    session.press("j <Enter>");
    assert!(session.text().contains("Change to practice on"));
}

#[test]
fn retry_after_a_failed_write_asks_again_only_for_the_area_that_failed() {
    let ai = Arc::new(ScriptedAi::new());
    let domain = stems("Domain", 3);
    let infrastructure = stems("Infrastructure", 3);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(domain
            .iter()
            .map(|stem| candidate(stem, "A", SERVE_OVER_HTTP))
            .collect()),
    );
    ai.on_write(
        &area_id(INFRASTRUCTURE),
        Err(RequestFailure::ProviderUnavailable),
    );
    ai.on_write(
        &area_id(INFRASTRUCTURE),
        Ok(infrastructure
            .iter()
            .map(|stem| candidate(stem, "A", SERVE_OVER_HTTP))
            .collect()),
    );
    for stem in domain.iter().chain(&infrastructure) {
        ai.on_check_passing(stem);
        reasoning_met(&ai, stem);
    }
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);
    session.settle();
    assert!(
        session.text().contains(
            "Writing questions failed: OpenRouter or the model's provider is unavailable."
        ),
        "{}",
        session.text()
    );

    session.press("<Enter>");
    session.settle();

    let asked = asked_for(&ai);
    assert_eq!(
        asked[..4],
        [
            "gate",
            "write domain",
            "write infrastructure",
            "write infrastructure"
        ],
        "neither the gate nor the domain area is asked again"
    );
    assert_eq!(
        asked.len(),
        28,
        "then every call of every candidate's check is sent once"
    );
    submit(&mut session, "a", "Because.");
    session.press("<Enter>");
    assert_eq!(
        session.question().row_with("Question 2"),
        "Question 2 of 4 · Infrastructure"
    );
}

#[test]
fn retry_after_a_failed_check_asks_again_only_for_the_call_that_failed() {
    // The answer probe, the fourth call, fails and is retried on its own
    // as the other three do.
    for call in QuestionCheckCall::ALL {
        let ai = Arc::new(ScriptedAi::new());
        let stems = stems("Domain", 6);
        ai.on_write(
            &area_id(DOMAIN),
            Ok(stems
                .iter()
                .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
                .collect()),
        );
        for (index, stem) in stems.iter().enumerate() {
            if index == 2 {
                ai.on_check_call_failing(stem, call, RequestFailure::TimedOut);
            }
            ai.on_check_passing(stem);
        }
        let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
        session.settle();
        let rows = rows(&session);
        assert!(
            rows.contains(&"Checking questions failed: timed out.".to_owned()),
            "{call:?}: {rows:#?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("✓ Writing questions · generator")),
            "{call:?}: {rows:#?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("✗ Checking questions · Jev")),
            "{call:?}: {rows:#?}"
        );
        let asked = ai.asked().len();

        session.press("<Enter>");
        session.settle();

        assert_eq!(
            asked_for(&ai)[asked..],
            [format!("check Domain question 3? ({call:?})")]
        );
        assert_eq!(
            session.question().row_with("Question 1"),
            "Question 1 of 4 · Domain",
            "{call:?}"
        );
    }
}

#[test]
fn with_an_ai_generate_questions_needs_a_change_and_an_area() {
    let ai = Arc::new(ScriptedAi::new());
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            ai: Some(&ai),
            ..Setup::default()
        },
        80,
        24,
    );
    session.press("<Enter>");
    assert_eq!(
        session.question().row_with("Generate questions"),
        "Generate questions (choose a change)"
    );
    session.press("<S-Tab> <Enter>");
    assert!(
        session.text().contains("Change to practice on"),
        "activating it does nothing yet"
    );

    session.press("<Tab> <Space> <Tab> <Space> <Tab> <Space> <Tab> <Space> <Tab> <Space>");
    assert_eq!(
        session.question().row_with("Generate questions"),
        "Generate questions (choose a change and an area)"
    );
    session.press("<Tab> <Space>");
    assert_eq!(
        session.question().row_with("Generate questions"),
        "Generate questions (choose an area)"
    );
    assert_eq!(ai.asked().len(), 0, "nothing is asked");
}
