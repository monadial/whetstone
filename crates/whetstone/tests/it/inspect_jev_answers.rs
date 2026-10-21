//! Inspect Jev's answers, opened from feedback: each of Jev's answers about
//! the question, from its question check, its reasoning's evaluation and
//! its explanation's check, with its probability as returned, or the depth's
//! distribution, its band under the assessment policy, the policy's
//! version and the models asked and answering. It is labelled "Jev's
//! probability, not a measure of correctness". Feedback itself shows no
//! numbers, and Jev's probabilities never reach the log.

use std::sync::Arc;

use whetstone_ai::{ParagraphCheck, ScriptedAi, WrittenParagraph};
use whetstone_core::{CandidateCheck, CriterionOutcome, Probability, WrittenCitation};

use crate::answering::{answering, submit};
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, stems, touched_path,
};
use crate::session::Session;

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// The reasoning the tests submit.
const REASONING: &str = "Retries reuse the key, so the provider dedupes them.";

/// A terminal height that fits all of the feedback.
const TALL: u16 = 60;

/// What labels Jev's numbers.
const LABEL: &str = "Jev's probability, not a measure of correctness";

/// Jev's answers the tests script, each one only it gives, so that a test
/// can tell where each shows.
const ANSWERABLE: f64 = 0.9731;
const NEEDS_CHANGE: f64 = 0.8462;
const CRITERION: f64 = 0.6214;
const SUPPORTED: f64 = 0.9317;
const AGREES: f64 = 0.8829;

fn probability(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, each passing every check, the first with
/// [`ANSWERABLE`] and [`NEEDS_CHANGE`] among its answers. Jev finds the
/// first's reasoning unclear on its one criterion, at [`CRITERION`], and
/// the generator's explanation of it passes Jev's check, at [`SUPPORTED`]
/// and [`AGREES`].
fn writing() -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    ai.on_check_passing_except(
        FIRST,
        [
            answer(CandidateCheck::Answerable, ANSWERABLE),
            answer(CandidateCheck::NeedsChange, NEEDS_CHANGE),
        ],
    );
    for stem in &stems[1..] {
        ai.on_check_passing(stem);
    }
    ai.on_assess(FIRST, Ok(vec![probability(CRITERION)]));
    ai.on_explain(
        FIRST,
        Ok(vec![WrittenParagraph {
            criterion: 0,
            outcome: CriterionOutcome::NeedsReview,
            text: "Meeting it would say the provider dedupes by key.".to_owned(),
            citations: vec![WrittenCitation {
                path: touched_path(PAYMENT_LIFECYCLE),
                first_line: 1,
                last_line: 1,
            }],
        }]),
    );
    ai.on_check_explanation(
        FIRST,
        Ok(vec![ParagraphCheck {
            supported: probability(SUPPORTED),
            agrees: probability(AGREES),
        }]),
    );
    ai
}

/// A session `width` by `height` on the feedback for a response choosing A
/// to the first question [`writing`] writes, with every result delivered.
fn on_feedback(width: u16, height: u16) -> Session {
    let mut session = answering(&writing(), width, height);
    submit(&mut session, "a", REASONING);
    session.settle();
    session
}

/// [`on_feedback`], then Inspect Jev's answers.
fn inspecting(width: u16, height: u16) -> Session {
    let mut session = on_feedback(width, height);
    activate(&mut session, "Inspect Jev's answers");
    session
}

/// Everything recorded in the log while `run` runs, at every level.
fn logged(run: impl FnOnce()) -> String {
    use std::io;
    use std::sync::Mutex;

    #[derive(Debug, Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("log lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = captured.0.lock().expect("log lock").clone();
    String::from_utf8(bytes).expect("the log is UTF-8")
}

/// The controls that show, in order, from the rows of `session`.
fn controls(session: &Session) -> Vec<String> {
    const LABELS: [&str; 6] = [
        "Retry explanation",
        "Next question",
        "Inspect source",
        "Inspect Jev's answers",
        "Report",
        "Finish",
    ];
    session
        .rows()
        .iter()
        .map(|row| row.trim().trim_start_matches("> ").to_owned())
        .filter(|row| LABELS.contains(&row.as_str()))
        .collect()
}

#[test]
fn feedback_offers_inspect_jevs_answers_after_inspect_source_and_before_report() {
    let session = on_feedback(120, TALL);

    assert_eq!(
        controls(&session),
        [
            "Next question",
            "Inspect source",
            "Inspect Jev's answers",
            "Report",
            "Finish"
        ]
    );
}

#[test]
fn inspect_shows_each_of_jevs_answers_with_its_band_the_policy_version_and_the_models() {
    let session = inspecting(120, TALL);

    assert_eq!(
        session.row_with("Jev's answers about"),
        "Jev's answers about question 1 of 4"
    );
    let models = "Asked scripted/jev, answered by scripted/jev · assessment policy 0";
    assert_eq!(
        session.section("Question check"),
        [
            models,
            "Answerable from the evidence sent · 0.9731 · clear yes",
            "About its area · 0.9 · clear yes",
            "Needs this change, not general knowledge · 0.8462 · clear yes",
            "The stem and options don't give the answer away · 0.9 · clear yes",
            "Criterion 1 follows from the evidence and the answer key · 0.9 · clear yes",
            "Option A answers the question correctly · 0.9 · clear yes",
            "Option B answers the question correctly · 0.1 · clear no",
            "Option C answers the question correctly · 0.1 · clear no",
            "The rationale for A is supported by the evidence · 0.9 · clear yes",
            "The rationale for B is supported by the evidence · 0.9 · clear yes",
            "The rationale for C is supported by the evidence · 0.9 · clear yes",
            "\"Why it matters\" is supported by the evidence · 0.9 · clear yes",
            "The hint points to the evidence · 0.9 · clear yes",
            "The hint doesn't reveal the answer key · 0.9 · clear yes",
            "Depth · 0.1 one line, 0.8 the change, 0.1 the code around it · expected level 1 · \
             not banded",
        ]
    );
    assert_eq!(
        session.section("Reasoning"),
        [
            models,
            "Meets criterion 1: The rubric's criterion, which stays hidden · 0.6214 · unclear",
        ]
    );
    assert_eq!(
        session.section("Explanation check"),
        [
            models,
            "Criterion 1's explanation is supported by the evidence · 0.9317 · clear yes",
            "Criterion 1's explanation agrees with its outcome · 0.8829 · clear yes",
        ]
    );
}

#[test]
fn inspect_is_labelled_jevs_probability_not_a_measure_of_correctness() {
    let session = inspecting(120, TALL);

    assert_eq!(
        session.section(LABEL),
        [
            "Each number is Jev's probability that the statement beside it holds, exactly as Jev \
             returned it: it is not a score",
            "of your answer. Assessment policy 0, unvalidated, bands each one clear yes, unclear or \
             clear no, and feedback says",
            "only what the bands lead to.",
        ]
    );
}

#[test]
fn inspect_before_jev_has_assessed_the_reasoning_says_it_awaits_evaluation() {
    let mut session = answering(&writing(), 120, TALL);
    submit(&mut session, "a", REASONING);

    activate(&mut session, "Inspect Jev's answers");

    assert_eq!(
        session.section("Reasoning"),
        ["Awaiting evaluation: Jev hasn't answered yet."]
    );
    assert!(
        !session.text().contains("Explanation check"),
        "{}",
        session.text()
    );
}

#[test]
fn back_from_inspect_returns_to_the_feedback() {
    let mut session = inspecting(120, TALL);

    session.press("q");

    assert!(
        session.text().contains("Choice · Supported"),
        "{}",
        session.text()
    );
}

#[test]
fn feedback_itself_shows_no_numbers() {
    let session = on_feedback(120, TALL);

    let text = session.text();
    assert!(text.contains("Reasoning"), "{text}");
    assert!(text.contains("Needs review"), "{text}");
    for (at, _) in text.match_indices('.') {
        let around = |offset: isize| {
            at.checked_add_signed(offset)
                .and_then(|place| text.as_bytes().get(place))
                .is_some_and(u8::is_ascii_digit)
        };
        assert!(!(around(-1) && around(1)), "a number at byte {at}:\n{text}");
    }
    assert!(!text.contains('%'), "{text}");
    for answer in [ANSWERABLE, NEEDS_CHANGE, CRITERION, SUPPORTED, AGREES] {
        let percent = format!("{:.0}", answer * 100.0);
        assert!(
            !text.contains(&percent),
            "{percent}, as a whole number:\n{text}"
        );
    }
}

#[test]
fn a_paragraph_checked_again_after_retry_explanation_names_each_check() {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
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
    ai.on_assess(FIRST, Ok(vec![probability(0.1)]));
    let paragraph = WrittenParagraph {
        criterion: 0,
        outcome: CriterionOutcome::NotMet,
        text: "You didn't say why reusing the key stops a second charge.".to_owned(),
        citations: vec![WrittenCitation {
            path: touched_path(PAYMENT_LIFECYCLE),
            first_line: 1,
            last_line: 1,
        }],
    };
    let check = |supported, agrees| ParagraphCheck {
        supported: probability(supported),
        agrees: probability(agrees),
    };
    ai.on_explain(FIRST, Ok(vec![paragraph.clone()]));
    ai.on_check_explanation(FIRST, Ok(vec![check(0.41, 0.9)]));
    ai.on_explain(FIRST, Ok(vec![paragraph]));
    ai.on_check_explanation(FIRST, Ok(vec![check(0.94, 0.9)]));
    let mut session = answering(&ai, 120, TALL);
    submit(&mut session, "a", REASONING);
    session.settle();
    activate(&mut session, "Retry explanation");
    session.settle();

    activate(&mut session, "Inspect Jev's answers");

    assert_eq!(
        session.section("Explanation check")[1..],
        [
            "Criterion 1's explanation, check 1 of 2, is supported by the evidence · 0.41 · unclear",
            "Criterion 1's explanation, check 1 of 2, agrees with its outcome · 0.9 · clear yes",
            "Criterion 1's explanation, check 2 of 2, is supported by the evidence · 0.94 · clear \
             yes",
            "Criterion 1's explanation, check 2 of 2, agrees with its outcome · 0.9 · clear yes",
        ]
    );
}

#[test]
fn jevs_probabilities_show_on_inspect_and_never_reach_the_log() {
    let mut text = String::new();

    let log = logged(|| {
        let session = inspecting(120, TALL);
        text = session.text();
    });

    for answer in [ANSWERABLE, NEEDS_CHANGE, CRITERION, SUPPORTED, AGREES] {
        let answer = answer.to_string();
        assert!(text.contains(&answer), "{answer} on Inspect:\n{text}");
        assert!(!log.contains(&answer), "{answer} reached the log:\n{log}");
    }
    assert!(log.contains("Jev assessed the reasoning"), "{log}");
}

#[test]
fn inspect_jevs_answers_at_120x32() {
    let session = inspecting(120, 32);

    insta::assert_debug_snapshot!("inspect_jevs_answers_120x32", session.frame());
}

#[test]
fn inspect_jevs_answers_at_80x24() {
    let session = inspecting(80, 24);

    insta::assert_debug_snapshot!("inspect_jevs_answers_80x24", session.frame());
}
