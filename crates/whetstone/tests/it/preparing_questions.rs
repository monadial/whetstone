//! Preparing questions on the showcase project, against a scripted AI:
//! Generate questions finds the change's connected code, then one generator
//! call per chosen area writes candidates, plain checks drop malformed ones,
//! Jev checks each one left, the show rule keeps the clear-cut ones and
//! picking chooses up to `questions` of them.

use std::sync::Arc;

use whetstone_ai::{Asked, RequestFailure, ScriptedAi};
use whetstone_core::{
    CandidateCheck, OptionLetter, Probability, QuestionCheckAnswers, QuestionType,
    WrittenCandidate, WrittenCitation, WrittenCriterion, WrittenOption,
};

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
pub(crate) fn area_id(place: usize) -> whetstone_core::AreaId {
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
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            ai: Some(ai),
            ..Setup::default()
        },
        width,
        height,
    );
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

/// Jev's answers to a single-choice question check: the probability that
/// each of A, B and C correctly answers the question.
pub(crate) fn answers(a: f64, b: f64, c: f64) -> QuestionCheckAnswers {
    QuestionCheckAnswers::new(OptionLetter::ALL.into_iter().zip([a, b, c]).map(
        |(letter, value)| {
            (
                CandidateCheck::Option(letter),
                Probability::new(value).expect("from 0 to 1"),
            )
        },
    ))
}

/// Answers that pass the show rule for a candidate whose answer key is A.
pub(crate) fn a_is_clearly_right() -> QuestionCheckAnswers {
    answers(0.9, 0.1, 0.05)
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
        ai.on_check(stem, Ok(a_is_clearly_right()));
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
    let [Asked::WriteCandidates { area, count, .. }, checks @ ..] = asked.as_slice() else {
        panic!("one write, then the checks: {asked:#?}");
    };
    assert_eq!(area.id(), &area_id(DOMAIN));
    assert_eq!(count.get(), 6, "4 questions and 2 more, for one area");
    assert_eq!(checks.len(), 6, "one check per candidate");
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
            ai.on_check(stem, Ok(a_is_clearly_right()));
        }
    }
    let mut session = generate(&ai, &[DOMAIN, INFRASTRUCTURE], SERVE_OVER_HTTP, 120, 32);

    session.settle_once();
    let writes: Vec<(String, usize)> = ai
        .asked()
        .iter()
        .map(|asked| match asked {
            Asked::WriteCandidates { area, count, .. } => (area.id().to_string(), count.get()),
            Asked::CheckQuestion { .. } => panic!("a check before every area is written"),
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
            Asked::WriteCandidates { .. } => None,
        })
        .collect();
    assert_eq!(
        checked,
        [domain.clone(), infrastructure.clone()].concat(),
        "every candidate checked before any check replies, in the order written"
    );
    assert!(session.text().contains("Preparing questions"));

    session.settle_once();
    let text = session.text();
    assert!(text.contains("Question 1 of 4 · Domain"), "{text}");
}

/// A scripted AI that writes `count` candidates about the domain area, each
/// answered clearly right at A, for the change at `change`.
pub(crate) fn writing_about_the_domain(count: usize, change: usize) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", count);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", change))
            .collect()),
    );
    for stem in &stems {
        ai.on_check(stem, Ok(a_is_clearly_right()));
    }
    ai
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

    insta::assert_debug_snapshot!("preparing_questions_writing_120x32", session.frame());
}

#[test]
fn preparing_questions_while_jev_checks_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
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
