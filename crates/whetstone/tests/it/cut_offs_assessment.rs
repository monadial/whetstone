//! The cut-offs in effect band every one of Jev's answers: policy 1's by
//! default, or the ones the learner's config sets under `[cut_offs]`. Each
//! outcome, each request's provenance and Inspect Jev's answers record the
//! cut-offs that banded it, and whether they are policy 1's or the
//! learner's own. With none set, nothing changes.

use std::sync::Arc;

use whetstone::{Assessment, ExplanationCheck, StoreEntry};
use whetstone_ai::{ParagraphCheck, RequestFailure, ScriptedAi, WrittenParagraph};
use whetstone_core::{
    AppliedCutOffs, AssessmentPolicy, CandidateCheck, CriterionOutcome, CutOffs, CutOffsOrigin,
    Job, OptionLetter, Probability, WrittenCitation,
};

use crate::answering::submit;
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, generate_in, reasoning_met, stems,
    touched_path,
};
use crate::questions_screen::every_stem_shown;
use crate::session::{NO_COLOR, Session, Setup};

/// The stem of the candidate each test checks, which is written first.
pub(crate) const TESTED: &str = "Tested question?";

/// A terminal height that fits all of the feedback and Inspect.
const TALL: u16 = 60;

/// A config that lowers the question check's upper cut-off to 0.55.
const LOWER_QUESTION_CHECK: &str = "[cut_offs]\nquestion_check = { upper = 0.55, lower = 0.20 }\n";

fn probability(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// The cut-offs of `job` under policy 1 with the learner's `upper` and
/// `lower` for it.
fn learner(job: Job, upper: f64, lower: f64) -> AppliedCutOffs {
    AssessmentPolicy::with_cut_offs([(job, CutOffs::new(upper, lower).expect("valid"))])
        .cut_offs(job)
}

/// A session, with `config` as the global config, that prepared the domain
/// area's questions on the payment lifecycle change from the tested
/// candidate, written first, and three that pass every check. Jev passes
/// the tested candidate's check but for `answers`, and the reasoning
/// submitted to each meets its criterion. Every result is delivered.
pub(crate) fn prepared(
    config: Option<&str>,
    answers: impl IntoIterator<Item = (CandidateCheck, Probability)>,
) -> Session {
    let ai = Arc::new(ScriptedAi::new());
    let others = stems("Domain", 3);
    let mut written = vec![candidate(TESTED, "A", PAYMENT_LIFECYCLE)];
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
    started(config, &ai)
}

/// A session, with `config` as the global config, that asked `ai` about
/// the domain area of the payment lifecycle change, with every result
/// delivered.
fn started(config: Option<&str>, ai: &Arc<ScriptedAi>) -> Session {
    let mut session = generate_in(
        Setup {
            config,
            variables: NO_COLOR,
            ai: Some(ai),
            ..Setup::default()
        },
        &[DOMAIN],
        PAYMENT_LIFECYCLE,
        120,
        TALL,
    );
    session.settle();
    session
}

/// Whether the tested candidate is shown, and the shortfall line says it
/// isn't, or else no shortfall line shows.
pub(crate) fn is_shown(session: &mut Session) -> bool {
    let shortfall = session.text().contains("3 passed checks, 4 asked for");
    let shown = every_stem_shown(session, &[TESTED.to_owned()]).contains(&TESTED.to_owned());
    assert_eq!(shown, !shortfall, "{}", session.text());
    shown
}

/// Every outcome of a response the store's `log` holds.
fn assessments(log: &[StoreEntry]) -> Vec<Assessment> {
    log.iter()
        .filter_map(|entry| match entry {
            StoreEntry::Assessment { assessment, .. } => Some(assessment.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_question_needing_the_change_at_0_6_is_withheld_under_policy_1_and_shown_under_the_learner_s_question_check()
 {
    let needs_change = [answer(CandidateCheck::NeedsChange, 0.6)];

    let mut defaults = prepared(None, needs_change);
    let mut lowered = prepared(Some(LOWER_QUESTION_CHECK), needs_change);

    assert_eq!(
        lowered.question().row_with("Question 1"),
        "Question 1 of 4 · Domain"
    );
    assert!(!is_shown(&mut defaults), "unclear under 0.80");
    assert!(is_shown(&mut lowered), "clear yes from 0.55");
}

#[test]
fn the_option_ratings_and_the_rationale_and_hint_checks_use_the_learner_s_cut_offs() {
    for (config, answers) in [
        (
            "option_rating = { upper = 0.80, lower = 0.30 }",
            answer(CandidateCheck::Option(OptionLetter::B), 0.25),
        ),
        (
            "rationale_and_hint_checks = { upper = 0.55, lower = 0.20 }",
            answer(CandidateCheck::Rationale(OptionLetter::C), 0.6),
        ),
    ] {
        let mut defaults = prepared(None, [answers]);
        let mut learners = prepared(Some(&format!("[cut_offs]\n{config}\n")), [answers]);

        assert!(!is_shown(&mut defaults), "{config}: unclear under policy 1");
        assert!(
            is_shown(&mut learners),
            "{config}: clear under the learner's"
        );
    }
}

#[test]
fn the_relevance_gate_uses_the_learner_s_cut_offs() {
    let gate = |ai: &ScriptedAi| {
        ai.on_gate(&area_id(DOMAIN), probability(0.3));
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
    };
    let (defaults, learners) = (Arc::new(ScriptedAi::new()), Arc::new(ScriptedAi::new()));
    gate(&defaults);
    gate(&learners);

    let unclear = started(None, &defaults);
    let clear_no = started(
        Some("[cut_offs]\nrelevance_gate = { upper = 0.80, lower = 0.35 }\n"),
        &learners,
    );

    assert!(
        !unclear.text().contains("Nothing to practice"),
        "{}",
        unclear.text()
    );
    assert!(
        clear_no
            .text()
            .contains("Nothing to practice in this change"),
        "{}",
        clear_no.text()
    );
}

#[test]
fn a_reasoning_criterion_is_met_under_the_learner_s_cut_offs_and_records_them() {
    let criterion_outcome = |config: Option<&str>| {
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
            ai.on_assess(stem, Ok(vec![probability(0.65)]));
            // Needs review asks for an explanation, which isn't tested here.
            ai.on_explain(stem, Err(RequestFailure::RateLimited));
        }
        let mut session = started(config, &ai);
        submit(&mut session, "a", "The key dedupes retries.");
        session.settle();
        assessments(&session.store().log())
            .into_iter()
            .find_map(|assessment| match assessment {
                Assessment::Criterion { outcome, .. } => Some(outcome),
                _ => None,
            })
            .expect("the criterion's outcome is saved")
    };

    let defaults = criterion_outcome(None);
    let learners = criterion_outcome(Some(
        "[cut_offs]\nreasoning_criterion = { upper = 0.60, lower = 0.20 }\n",
    ));

    assert_eq!(defaults.outcome(), CriterionOutcome::NeedsReview);
    assert_eq!(
        defaults.cut_offs(),
        AssessmentPolicy::current().cut_offs(Job::ReasoningCriterion)
    );
    assert_eq!(learners.outcome(), CriterionOutcome::Met);
    assert_eq!(
        learners.cut_offs(),
        learner(Job::ReasoningCriterion, 0.60, 0.20)
    );
    assert_eq!(learners.cut_offs().origin(), CutOffsOrigin::Learner);
    assert_eq!(learners.policy().get(), 1);
}

#[test]
fn an_explanation_paragraph_shows_under_the_learner_s_cut_offs_and_records_them() {
    let checked = |config: Option<&str>| {
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
        ai.on_assess(&stems[0], Ok(vec![probability(0.5)]));
        ai.on_explain(
            &stems[0],
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
            &stems[0],
            Ok(vec![ParagraphCheck {
                supported: probability(0.7),
                agrees: probability(0.9),
            }]),
        );
        let mut session = started(config, &ai);
        submit(&mut session, "a", "The key dedupes retries.");
        session.settle();
        assessments(&session.store().log())
            .into_iter()
            .find_map(|assessment| match assessment {
                Assessment::Explanation { check, .. } => Some(check),
                _ => None,
            })
            .expect("the paragraph's check is saved")
    };

    let defaults = checked(None);
    let learners = checked(Some(
        "[cut_offs]\nexplanation_check = { upper = 0.65, lower = 0.20 }\n",
    ));

    let answers = ParagraphCheck {
        supported: probability(0.7),
        agrees: probability(0.9),
    };
    assert_eq!(
        defaults,
        ExplanationCheck::Failed {
            answers,
            cut_offs: AssessmentPolicy::current().cut_offs(Job::ExplanationCheck),
        }
    );
    assert_eq!(
        learners,
        ExplanationCheck::Passed {
            answers,
            cut_offs: learner(Job::ExplanationCheck, 0.65, 0.20),
        }
    );
}

#[test]
fn the_record_holds_the_cut_offs_each_question_was_checked_with_and_each_request_went_out_with() {
    let mut session = prepared(
        Some(LOWER_QUESTION_CHECK),
        [answer(CandidateCheck::NeedsChange, 0.6)],
    );
    submit(&mut session, "a", "The key dedupes retries.");
    session.settle();

    let log = session.store().log();
    let record = log
        .iter()
        .find_map(|entry| match entry {
            StoreEntry::Started { record, .. } => Some(record.clone()),
            _ => None,
        })
        .expect("the record started");
    let policy = AssessmentPolicy::with_cut_offs([(
        Job::QuestionCheck,
        CutOffs::new(0.55, 0.20).expect("valid"),
    )]);
    assert!(
        record
            .questions
            .iter()
            .any(|question| question.stem() == TESTED)
    );
    for question in &record.questions {
        assert_eq!(question.policy(), &policy, "{}", question.stem());
    }
    let requests: Vec<_> = record
        .requests
        .iter()
        .chain(log.iter().filter_map(|entry| match entry {
            StoreEntry::Request { request, .. } => Some(request),
            _ => None,
        }))
        .collect();
    assert!(
        requests.len() >= 6,
        "the gate, writing, four checks and more"
    );
    for request in requests {
        assert_eq!(
            request.provenance.versions().policy(),
            &policy,
            "{:?}",
            request.asking
        );
    }
    let outcomes = assessments(&log);
    let Some(Assessment::Choice(choice)) = outcomes.first() else {
        panic!("the choice's outcome is saved first: {outcomes:?}");
    };
    assert_eq!(choice.cut_offs(), policy.cut_offs(Job::OptionRating));
    assert_eq!(choice.cut_offs().origin(), CutOffsOrigin::Default);
}

#[test]
fn inspect_shows_the_cut_offs_that_banded_each_answer_and_whose_they_are() {
    let mut session = prepared(
        Some(LOWER_QUESTION_CHECK),
        [answer(CandidateCheck::NeedsChange, 0.6)],
    );
    assert!(
        session.text().contains(TESTED),
        "the tested question shows first: {}",
        session.text()
    );
    submit(&mut session, "a", "The key dedupes retries.");
    session.settle();

    activate(&mut session, "Inspect Jev's answers");

    let check = session.question().section("Question check");
    for line in [
        "Cut-offs for the question check · upper 0.55, lower 0.2 · from your config",
        "Cut-offs for the giveaway check · upper 0.5, lower 0.2 · policy 1's",
        "Cut-offs for the option rating · upper 0.8, lower 0.2 · policy 1's",
        "Cut-offs for the rationale and hint checks · upper 0.8, lower 0.2 · policy 1's",
        "Needs this change, not general knowledge · 0.6 · clear yes",
    ] {
        assert!(check.contains(&line.to_owned()), "{line}: {check:#?}");
    }
    let reasoning = session.question().section("Reasoning");
    assert!(
        reasoning.contains(
            &"Cut-offs for the reasoning criterion · upper 0.8, lower 0.2 · policy 1's".to_owned()
        ),
        "{reasoning:#?}"
    );
    let words = session
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        words.contains(
            "Your config changes some of its cut-offs, so these bands aren't comparable with \
             policy 1's."
        ),
        "{words}"
    );
}

#[test]
fn with_every_job_at_its_default_inspect_names_policy_1_s_cut_offs_and_nothing_else_changes() {
    for config in [
        None,
        Some("[cut_offs]\nquestion_check = { upper = 0.80, lower = 0.20 }\n"),
    ] {
        let mut session = prepared(config, []);
        submit(&mut session, "a", "The key dedupes retries.");
        session.settle();

        let log = session.store().log();
        for request in log.iter().filter_map(|entry| match entry {
            StoreEntry::Started { record, .. } => Some(&record.requests),
            _ => None,
        }) {
            for sent in request {
                assert_eq!(
                    sent.provenance.versions().policy(),
                    AssessmentPolicy::current(),
                    "{config:?}"
                );
            }
        }
        activate(&mut session, "Inspect Jev's answers");
        assert!(
            session.question().section("Question check").contains(
                &"Cut-offs for the question check · upper 0.8, lower 0.2 · policy 1's".to_owned()
            ),
            "{config:?}: {}",
            session.text()
        );
        assert!(
            !session.text().contains("Your config changes"),
            "{config:?}"
        );
    }
}
