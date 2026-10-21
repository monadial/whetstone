//! The explanation of what the reasoning missed. Once Jev has assessed the
//! reasoning, the generator writes one paragraph, with citations, for each
//! criterion not met or needing review, and none when every criterion is
//! met. Plain code checks the citations, then Jev checks every paragraph
//! left: supported by the evidence, and agreeing with its criterion's
//! outcome. A paragraph shows under its criterion's outcome, labelled as
//! the generator's, only when both answers are clear yes. Otherwise it is
//! withheld, with Retry explanation, and an explanation that fails to
//! arrive is unavailable, with Retry explanation. The assessment always
//! stays, and every paragraph checked is saved to the session record.

use std::sync::Arc;

use whetstone::{Assessment, ExplanationCheck, SavedResponse, StoreEntry};
use whetstone_ai::{Asked, ParagraphCheck, RequestFailure, ScriptedAi, WrittenParagraph};
use whetstone_core::{
    AssessmentPolicy, CriterionOutcome, Probability, WrittenCandidate, WrittenCitation,
    WrittenCriterion,
};

use crate::answering::{answering, submit};
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, stems, touched_path,
};
use crate::session::Session;

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// A terminal height that fits all of the feedback, for the tests that
/// read it whole.
const TALL: u16 = 60;

/// The reasoning the tests submit.
const REASONING: &str = "Retries reuse the key, so the provider dedupes them.";

/// The first question's rubric, in order.
const CRITERIA: [&str; 3] = [
    "Names the key the retry reuses",
    "Says why the retry can't charge twice",
    "Says what the timed-out charge may already have done",
];

/// What the generator explains for the second criterion, which isn't met.
const NOT_MET: &str = "You didn't say why reusing the key stops a second charge.";

/// What the generator explains for the third criterion, which needs review.
const NEEDS_REVIEW: &str = "Meeting it would say the timed-out charge may have gone through.";

fn probability(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, each passing every check, its answer key A, the
/// first with [`CRITERIA`] as its rubric. Jev assesses the reasoning written for the
/// first as `assessment`: a probability per criterion.
fn writing(assessment: [f64; 3]) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    let mut written: Vec<WrittenCandidate> = stems
        .iter()
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written[0].rubric = CRITERIA
        .iter()
        .map(|&statement| WrittenCriterion {
            statement: statement.to_owned(),
            met: format!("Meeting it: {statement}"),
            not_met: format!("Not meeting it: {statement}"),
        })
        .collect();
    ai.on_write(&area_id(DOMAIN), Ok(written));
    for stem in &stems {
        ai.on_check_passing(stem);
    }
    ai.on_assess(FIRST, Ok(assessment.map(probability).to_vec()));
    ai
}

/// A paragraph explaining the criterion at `criterion`, whose outcome is
/// `outcome`, saying `text` and citing the first line of the file the
/// change touches, which the evidence sends.
fn paragraph(criterion: usize, outcome: CriterionOutcome, text: &str) -> WrittenParagraph {
    WrittenParagraph {
        criterion,
        outcome,
        text: text.to_owned(),
        citations: vec![WrittenCitation {
            path: touched_path(PAYMENT_LIFECYCLE),
            first_line: 1,
            last_line: 1,
        }],
    }
}

/// Jev's answers about one paragraph: supported by the evidence, and
/// agreeing with its criterion's outcome.
fn check(supported: f64, agrees: f64) -> ParagraphCheck {
    ParagraphCheck {
        supported: probability(supported),
        agrees: probability(agrees),
    }
}

/// A session `width` by `height` on the feedback for a response choosing A
/// to the first question that `ai` writes, with every result delivered.
fn explained(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    let mut session = answering(ai, width, height);
    submit(&mut session, "a", REASONING);
    session.settle();
    session
}

/// Every explanation `ai` was asked for, and every check of one, in order.
fn explaining(ai: &ScriptedAi) -> Vec<Asked> {
    ai.asked()
        .into_iter()
        .filter(|asked| {
            matches!(
                asked,
                Asked::Explain { .. } | Asked::CheckExplanation { .. }
            )
        })
        .collect()
}

#[test]
fn each_paragraph_that_passes_its_checks_shows_under_its_criterions_outcome_as_the_generators() {
    let ai = writing([0.9, 0.1, 0.5]);
    let written = [
        paragraph(1, CriterionOutcome::NotMet, NOT_MET),
        paragraph(2, CriterionOutcome::NeedsReview, NEEDS_REVIEW),
    ];
    ai.on_explain(FIRST, Ok(written.to_vec()));
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.85), check(0.95, 0.8)]));

    let session = explained(&ai, 120, TALL);

    let cited = format!("Cited {}, line 1", touched_path(PAYMENT_LIFECYCLE));
    assert_eq!(
        session.section("Reasoning"),
        [
            "Met · Names the key the retry reuses",
            "Not met · Says why the retry can't charge twice",
            "Explanation · generator",
            NOT_MET,
            &cited,
            "Needs review · Says what the timed-out charge may already have done",
            "Jev couldn't tell whether your reasoning covers this.",
            "Explanation · generator",
            NEEDS_REVIEW,
            &cited,
        ]
    );
    let [
        Asked::Explain {
            stem,
            reasoning,
            criteria,
            ..
        },
        Asked::CheckExplanation { paragraphs, .. },
    ] = &explaining(&ai)[..]
    else {
        panic!("one explanation, then one check: {:#?}", ai.asked());
    };
    assert_eq!(stem, FIRST);
    assert_eq!(reasoning, REASONING, "the reasoning as saved");
    assert_eq!(
        criteria,
        &[
            (1, CriterionOutcome::NotMet),
            (2, CriterionOutcome::NeedsReview)
        ],
        "the criteria not met or needing review, with their outcomes"
    );
    assert_eq!(paragraphs, &written, "every paragraph, in one check");
    let text = session.text();
    for number in ["0.9", "0.85", "0.95", "0.8", "%"] {
        assert!(!text.contains(number), "{number} shows:\n{text}");
    }
}

#[test]
fn a_paragraph_jev_isnt_clear_about_is_withheld_with_retry() {
    let ai = writing([0.9, 0.1, 0.9]);
    ai.on_explain(
        FIRST,
        Ok(vec![paragraph(1, CriterionOutcome::NotMet, NOT_MET)]),
    );
    // Supported, but unclear whether it agrees with its outcome.
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.5)]));

    let session = explained(&ai, 120, TALL);

    assert_eq!(
        session.section("Reasoning"),
        [
            "Met · Names the key the retry reuses",
            "Not met · Says why the retry can't charge twice",
            "Explanation withheld: it didn't pass checks",
            "Met · Says what the timed-out charge may already have done",
        ],
        "the outcomes stay, and the paragraph never shows"
    );
    assert_eq!(session.row_with("Retry explanation"), "Retry explanation");
    assert!(!session.text().contains(NOT_MET), "{}", session.text());
}

#[test]
fn a_paragraph_citing_lines_the_evidence_doesnt_send_is_withheld_before_jev_is_asked() {
    let ai = writing([0.1, 0.1, 0.9]);
    let mut uncited = paragraph(0, CriterionOutcome::NotMet, "You didn't name the key.");
    uncited.citations[0].path = "src/not_sent.rs".to_owned();
    let cited = paragraph(1, CriterionOutcome::NotMet, NOT_MET);
    ai.on_explain(FIRST, Ok(vec![uncited, cited.clone()]));
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.9)]));

    let session = explained(&ai, 120, TALL);

    assert_eq!(
        session.section("Reasoning")[..5],
        [
            "Not met · Names the key the retry reuses",
            "Explanation withheld: it didn't pass checks",
            "Not met · Says why the retry can't charge twice",
            "Explanation · generator",
            NOT_MET,
        ]
    );
    let checked: Vec<Vec<WrittenParagraph>> = explaining(&ai)
        .into_iter()
        .filter_map(|asked| match asked {
            Asked::CheckExplanation { paragraphs, .. } => Some(paragraphs),
            _ => None,
        })
        .collect();
    assert_eq!(
        checked,
        [vec![cited]],
        "only the paragraph that cites the evidence"
    );
}

#[test]
fn an_explanation_that_fails_to_arrive_is_unavailable_and_the_assessment_stays() {
    let ai = writing([0.9, 0.1, 0.5]);
    ai.on_explain(FIRST, Err(RequestFailure::RateLimited));

    let session = explained(&ai, 120, TALL);

    assert_eq!(
        session.section("Reasoning"),
        [
            "Met · Names the key the retry reuses",
            "Not met · Says why the retry can't charge twice",
            "Explanation unavailable: the generator's request failed: rate limited.",
            "Needs review · Says what the timed-out charge may already have done",
            "Jev couldn't tell whether your reasoning covers this.",
            "Explanation unavailable: the generator's request failed: rate limited.",
        ]
    );
    assert_eq!(session.row_with("Retry explanation"), "Retry explanation");
    assert_eq!(
        explaining(&ai).len(),
        1,
        "no check of an explanation that didn't come"
    );
}

#[test]
fn a_check_that_fails_leaves_the_explanation_unavailable() {
    let ai = writing([0.9, 0.1, 0.9]);
    ai.on_explain(
        FIRST,
        Ok(vec![paragraph(1, CriterionOutcome::NotMet, NOT_MET)]),
    );
    ai.on_check_explanation(FIRST, Err(RequestFailure::TimedOut));

    let session = explained(&ai, 120, TALL);

    assert_eq!(
        session.section("Reasoning")[1..3],
        [
            "Not met · Says why the retry can't charge twice",
            "Explanation unavailable: Jev's check failed: timed out.",
        ]
    );
    assert!(!session.text().contains(NOT_MET), "{}", session.text());
}

#[test]
fn an_explanation_of_other_criteria_than_asked_is_unavailable_never_shown() {
    let ai = writing([0.9, 0.1, 0.9]);
    ai.on_explain(
        FIRST,
        Ok(vec![paragraph(2, CriterionOutcome::NotMet, NOT_MET)]),
    );

    let session = explained(&ai, 120, TALL);

    assert_eq!(
        session.section("Reasoning")[2],
        "Explanation unavailable: the generator's request failed: the reply couldn't be read."
    );
    assert!(!session.text().contains(NOT_MET), "{}", session.text());
}

#[test]
fn retry_asks_again_only_for_what_didnt_show_and_what_showed_stays() {
    let ai = writing([0.9, 0.1, 0.5]);
    let shown = paragraph(1, CriterionOutcome::NotMet, NOT_MET);
    let withheld = paragraph(2, CriterionOutcome::NeedsReview, "You missed it.");
    let rewritten = paragraph(2, CriterionOutcome::NeedsReview, NEEDS_REVIEW);
    ai.on_explain(FIRST, Ok(vec![shown.clone(), withheld]));
    // Claims the learner missed a criterion that needs review: it doesn't
    // agree with the outcome.
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.9), check(0.9, 0.1)]));
    ai.on_explain(FIRST, Ok(vec![rewritten.clone()]));
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.9)]));
    let mut session = explained(&ai, 120, TALL);
    assert!(
        session
            .section("Reasoning")
            .contains(&"Explanation withheld: it didn't pass checks".to_owned()),
        "{}",
        session.text()
    );

    // Retry explanation is just above Next question, where focus starts.
    session.press("k <Enter>");
    assert!(
        session
            .text()
            .contains("The generator is explaining this. Jev checks it before it shows."),
        "{}",
        session.text()
    );
    assert!(
        !session.text().contains("Retry explanation"),
        "no Retry while it runs"
    );
    session.settle();

    let cited = format!("Cited {}, line 1", touched_path(PAYMENT_LIFECYCLE));
    assert_eq!(
        session.section("Reasoning")[1..],
        [
            "Not met · Says why the retry can't charge twice",
            "Explanation · generator",
            NOT_MET,
            &cited,
            "Needs review · Says what the timed-out charge may already have done",
            "Jev couldn't tell whether your reasoning covers this.",
            "Explanation · generator",
            NEEDS_REVIEW,
            &cited,
        ]
    );
    let asked = explaining(&ai);
    let Asked::Explain { criteria, .. } = &asked[2] else {
        panic!("the Retry asks the generator again: {asked:#?}");
    };
    assert_eq!(criteria, &[(2, CriterionOutcome::NeedsReview)]);
    assert!(
        matches!(&asked[3], Asked::CheckExplanation { paragraphs, .. } if paragraphs == &[rewritten]),
        "then Jev checks only the new paragraph: {asked:#?}"
    );
    assert!(!session.text().contains("Retry explanation"));
}

#[test]
fn there_is_no_explanation_when_every_criterion_is_met() {
    let ai = writing([0.9, 0.95, 0.8]);

    let session = explained(&ai, 120, TALL);

    assert_eq!(
        session.section("Reasoning"),
        CRITERIA.map(|criterion| format!("Met · {criterion}"))
    );
    assert_eq!(explaining(&ai), [], "no explanation call");
    assert!(
        !session.text().contains("Explanation"),
        "{}",
        session.text()
    );
}

/// Every explanation paragraph the store saved, with the response it
/// explains and what its checks made of it, in order.
fn saved_explanations(
    log: &[StoreEntry],
) -> Vec<(SavedResponse, WrittenParagraph, ExplanationCheck)> {
    log.iter()
        .filter_map(|entry| match entry {
            StoreEntry::Assessment {
                saved,
                assessment: Assessment::Explanation { paragraph, check },
            } => Some((saved.clone(), paragraph.clone(), *check)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_paragraph_checked_is_saved_to_the_session_record_shown_or_withheld() {
    let ai = writing([0.1, 0.1, 0.5]);
    let mut uncited = paragraph(0, CriterionOutcome::NotMet, "You didn't name the key.");
    uncited.citations[0].first_line = 1000;
    let shown = paragraph(1, CriterionOutcome::NotMet, NOT_MET);
    let withheld = paragraph(2, CriterionOutcome::NeedsReview, NEEDS_REVIEW);
    ai.on_explain(
        FIRST,
        Ok(vec![uncited.clone(), shown.clone(), withheld.clone()]),
    );
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.85), check(0.6, 0.9)]));

    let session = explained(&ai, 120, TALL);

    let log = session.store().log();
    let saved = log
        .iter()
        .find_map(|entry| match entry {
            StoreEntry::Response { saved, .. } => Some(saved.clone()),
            _ => None,
        })
        .expect("the response is saved");
    let policy = AssessmentPolicy::current().version();
    assert_eq!(
        saved_explanations(&log),
        [
            (saved.clone(), uncited, ExplanationCheck::FailedPlainChecks),
            (
                saved.clone(),
                shown,
                ExplanationCheck::Passed {
                    answers: check(0.9, 0.85),
                    policy,
                },
            ),
            (
                saved,
                withheld,
                ExplanationCheck::Failed {
                    answers: check(0.6, 0.9),
                    policy,
                },
            ),
        ]
    );
    let outcomes = log
        .iter()
        .position(|entry| {
            matches!(
                entry,
                StoreEntry::Assessment {
                    assessment: Assessment::Criterion { criterion: 2, .. },
                    ..
                }
            )
        })
        .expect("the criterion outcomes are saved");
    let first = log
        .iter()
        .position(|entry| {
            matches!(
                entry,
                StoreEntry::Assessment {
                    assessment: Assessment::Explanation { .. },
                    ..
                }
            )
        })
        .expect("the explanation is saved");
    assert!(outcomes < first, "the outcomes come first: {log:#?}");
}

#[test]
fn an_explanation_after_leaving_the_practice_changes_nothing() {
    let ai = writing([0.9, 0.1, 0.9]);
    ai.on_explain(
        FIRST,
        Ok(vec![paragraph(1, CriterionOutcome::NotMet, NOT_MET)]),
    );
    let mut session = answering(&ai, 120, TALL);
    submit(&mut session, "a", REASONING);
    // Delivers the assessment, which asks for the explanation.
    session.settle_once();
    assert_eq!(explaining(&ai).len(), 1, "the explanation was asked for");

    session.press("q");
    let left = session.text();
    session.settle();

    assert_eq!(session.text(), left, "the late explanation changes nothing");
    assert_eq!(explaining(&ai).len(), 1, "and asks Jev nothing");
    assert_eq!(saved_explanations(&session.store().log()), []);
}

/// A session `width` by `height` on feedback whose second criterion is not
/// met and whose third needs review, each explained, with the paragraph
/// for the second passing its checks and the one for the third passing them
/// if `third_passes`, scrolled to the bottom.
fn feedback_with_an_explanation(third_passes: bool, width: u16, height: u16) -> Session {
    let ai = writing([0.9, 0.1, 0.5]);
    ai.on_explain(
        FIRST,
        Ok(vec![
            paragraph(1, CriterionOutcome::NotMet, NOT_MET),
            paragraph(2, CriterionOutcome::NeedsReview, NEEDS_REVIEW),
        ]),
    );
    let third = if third_passes {
        check(0.9, 0.9)
    } else {
        check(0.9, 0.1)
    };
    ai.on_check_explanation(FIRST, Ok(vec![check(0.9, 0.85), third]));
    let mut session = explained(&ai, width, height);
    session.press("G");
    session
}

#[test]
fn an_explanation_shown_at_120x32() {
    let session = feedback_with_an_explanation(true, 120, 32);

    insta::assert_debug_snapshot!("explanation_shown_120x32", session.frame());
}

#[test]
fn an_explanation_shown_at_80x24() {
    let session = feedback_with_an_explanation(true, 80, 24);

    insta::assert_debug_snapshot!("explanation_shown_80x24", session.frame());
}

#[test]
fn an_explanation_withheld_at_120x32() {
    let session = feedback_with_an_explanation(false, 120, 32);

    insta::assert_debug_snapshot!("explanation_withheld_120x32", session.frame());
}

#[test]
fn an_explanation_withheld_at_80x24() {
    let session = feedback_with_an_explanation(false, 80, 24);

    insta::assert_debug_snapshot!("explanation_withheld_80x24", session.frame());
}
