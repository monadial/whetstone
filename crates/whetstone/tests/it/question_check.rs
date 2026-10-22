//! The full question check, against a scripted AI: a candidate is shown
//! only when Jev answers clear yes to every yes/no check apart from its
//! hint's and its area's (answerable from the evidence that was sent, each
//! criterion, each rationale and "why it matters"), doesn't clearly say it
//! is off its area, its option ratings pick out its answer key alone, and
//! the answer probe isn't clear yes on its answer key: Jev can't
//! confidently choose the answer without reading the change. Anything else
//! unclear withholds it. A hint that fails its checks never withholds its
//! question.

use std::sync::Arc;

use whetstone_ai::ScriptedAi;
use whetstone_core::{CandidateCheck, OptionLetter, Probability, WrittenCandidate};

use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, generate, reasoning_met, stems,
};
use crate::questions_screen::every_stem_shown;
use crate::session::Session;

/// The stem of the candidate each test checks.
const TESTED: &str = "Tested question?";

/// A session that prepared the domain area's questions on the payment
/// lifecycle change from `tested` and three candidates that pass every
/// check. Jev passes `tested`'s check but for `answers`, and the reasoning
/// submitted to each meets its criterion.
fn prepared(
    tested: WrittenCandidate,
    answers: impl IntoIterator<Item = (CandidateCheck, Probability)>,
) -> Session {
    let ai = Arc::new(ScriptedAi::new());
    let others = stems("Domain", 3);
    let mut written = vec![tested];
    written.extend(
        others
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE)),
    );
    ai.on_write(&area_id(DOMAIN), Ok(written));
    ai.on_check_passing_except(TESTED, answers);
    reasoning_met(&ai, TESTED);
    for stem in &others {
        ai.on_check_passing(stem);
        reasoning_met(&ai, stem);
    }
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    session.settle();
    session
}

/// The tested candidate as [`candidate`] writes it: answer key A, one
/// criterion.
fn tested() -> WrittenCandidate {
    candidate(TESTED, "A", PAYMENT_LIFECYCLE)
}

/// Whether the tested candidate is among the questions shown.
fn is_shown(session: &mut Session) -> bool {
    every_stem_shown(session, &[TESTED.to_owned()]).contains(&TESTED.to_owned())
}

/// Asserts that `tested` is withheld when Jev's answer to `check` is
/// unclear, just under the clear-yes cut-off, or clear no, and every other
/// check passes: only the three others are shown, and the shortfall says
/// so.
fn assert_withheld_unless_clear_yes(tested: &WrittenCandidate, check: CandidateCheck) {
    assert_withheld_at(tested, check, &[0.79, 0.5, 0.21, 0.1]);
}

/// Asserts that `tested` is withheld when Jev's answer to `check` is each
/// of `values`, and every other check passes: only the three others are
/// shown, and the shortfall says so.
fn assert_withheld_at(tested: &WrittenCandidate, check: CandidateCheck, values: &[f64]) {
    for &value in values {
        let mut session = prepared(tested.clone(), [answer(check, value)]);

        assert_eq!(
            session.question().rows()[1].trim(),
            "3 passed checks, 4 asked for · see why in actions",
            "{check:?} at {value}"
        );
        assert!(!is_shown(&mut session), "{check:?} at {value}");
    }
}

/// Asserts that `tested` is shown, with no shortfall, when Jev's answer to
/// `check` is each of `values`, and every other check passes.
fn assert_shown_at(tested: &WrittenCandidate, check: CandidateCheck, values: &[f64]) {
    for &value in values {
        let mut session = prepared(tested.clone(), [answer(check, value)]);

        assert!(is_shown(&mut session), "{check:?} at {value}");
        assert!(
            !session.text().contains("asked for"),
            "no shortfall at {value}: {}",
            session.text()
        );
    }
}

#[test]
fn a_question_that_passes_every_check_is_shown() {
    let mut session = prepared(tested(), []);

    assert_eq!(
        session.question().row_with("Question 1"),
        "Question 1 of 4 · Domain"
    );
    assert!(!session.text().contains("asked for"), "{}", session.text());
    assert!(is_shown(&mut session));
}

#[test]
fn a_question_not_answerable_from_the_evidence_that_was_sent_is_withheld() {
    assert_withheld_unless_clear_yes(&tested(), CandidateCheck::Answerable);
}

#[test]
fn a_question_jev_clearly_says_is_off_its_area_is_withheld() {
    // Clear no from 0.20 down.
    assert_withheld_at(&tested(), CandidateCheck::AboutArea, &[0.20, 0.19, 0.0]);
}

#[test]
fn a_question_whose_only_unclear_check_is_about_its_area_is_shown() {
    // Unclear from 0.79 down to 0.21: whether a question fits its area is a
    // matter of focus, so only a clear no withholds it.
    assert_shown_at(&tested(), CandidateCheck::AboutArea, &[0.79, 0.5, 0.21]);
}

#[test]
fn a_question_jev_answers_without_reading_the_change_is_withheld() {
    // Clear yes on the answer key, from 0.60.
    assert_withheld_at(&tested(), CandidateCheck::AnswerProbe, &[0.60, 0.75, 1.0]);
}

#[test]
fn a_question_jev_cannot_confidently_answer_without_the_change_is_shown() {
    // Unclear or clear no on the answer key: the other options share the
    // rest, so at 0.05 Jev picks one of them.
    assert_shown_at(
        &tested(),
        CandidateCheck::AnswerProbe,
        &[0.59, 0.33, 0.20, 0.05],
    );
}

#[test]
fn a_question_with_any_criterion_that_doesnt_hold_is_withheld() {
    let mut two_criteria = tested();
    let second = two_criteria.rubric[0].clone();
    two_criteria.rubric.push(second);

    assert_withheld_unless_clear_yes(&two_criteria, CandidateCheck::Criterion(0));
    assert_withheld_unless_clear_yes(&two_criteria, CandidateCheck::Criterion(1));
}

#[test]
fn a_question_with_any_rationale_the_evidence_doesnt_support_is_withheld() {
    for letter in OptionLetter::ALL {
        assert_withheld_unless_clear_yes(&tested(), CandidateCheck::Rationale(letter));
    }
}

#[test]
fn a_question_whose_why_it_matters_the_evidence_doesnt_support_is_withheld() {
    assert_withheld_unless_clear_yes(&tested(), CandidateCheck::WhyItMatters);
}

#[test]
fn a_question_whose_checks_are_all_at_the_cut_offs_is_shown() {
    use CandidateCheck::*;

    let mut session = prepared(
        tested(),
        [
            (Answerable, 0.80),
            (AboutArea, 0.21),
            (AnswerProbe, 0.59),
            (Criterion(0), 0.80),
            (Option(OptionLetter::A), 0.80),
            (Option(OptionLetter::B), 0.20),
            (Option(OptionLetter::C), 0.20),
            (Rationale(OptionLetter::A), 0.80),
            (Rationale(OptionLetter::B), 0.80),
            (Rationale(OptionLetter::C), 0.80),
            (WhyItMatters, 0.80),
            (HintPointsToEvidence, 0.80),
            (HintHidesAnswerKey, 0.80),
        ]
        .map(|(check, value)| answer(check, value)),
    );

    assert!(is_shown(&mut session));
    assert!(!session.text().contains("asked for"), "{}", session.text());
}

#[test]
fn a_question_whose_hint_fails_either_of_its_checks_is_still_shown() {
    for (points_to_evidence, hides_answer_key) in [(0.5, 0.9), (0.9, 0.1), (0.79, 0.21), (0.0, 0.0)]
    {
        let mut session = prepared(
            tested(),
            [
                answer(CandidateCheck::HintPointsToEvidence, points_to_evidence),
                answer(CandidateCheck::HintHidesAnswerKey, hides_answer_key),
            ],
        );

        assert!(
            is_shown(&mut session),
            "{points_to_evidence}, {hides_answer_key}"
        );
        assert!(
            !session.text().contains("asked for"),
            "no shortfall: {}",
            session.text()
        );
    }
}
