//! The giveaway check's own cut-offs: policy 1 bands `no_giveaway` at 0.50
//! and 0.20, apart from the rest of the question check at 0.80 and 0.20.
//! A learner's `giveaway_check` in `[cut_offs]` overrides them, and their
//! `question_check` no longer covers `no_giveaway`.

use whetstone::StoreEntry;
use whetstone_core::{AssessmentPolicy, CandidateCheck, CutOffs, CutOffsOrigin, Job};

use crate::answering::submit;
use crate::cut_offs_assessment::{TESTED, is_shown, prepared};
use crate::help_and_signals::activate;
use crate::preparing_questions::answer;
use crate::session::Session;

/// The policy the questions in the record of `session` were checked with.
fn recorded_policy(session: &Session) -> AssessmentPolicy {
    session
        .store()
        .log()
        .iter()
        .find_map(|entry| match entry {
            StoreEntry::Started { record, .. } => record
                .questions
                .first()
                .map(|question| question.policy().clone()),
            _ => None,
        })
        .expect("the record holds the questions shown")
}

fn cut_offs(upper: f64, lower: f64) -> CutOffs {
    CutOffs::new(upper, lower).expect("valid cut-offs")
}

#[test]
fn a_candidate_whose_only_unclear_check_is_no_giveaway_at_0_55_is_shown_under_the_defaults() {
    let mut giveaway = prepared(None, [answer(CandidateCheck::NoGiveaway, 0.55)]);
    let mut needs_change = prepared(None, [answer(CandidateCheck::NeedsChange, 0.55)]);

    assert!(is_shown(&mut giveaway), "clear yes from 0.50");
    assert!(!is_shown(&mut needs_change), "unclear under 0.80");
}

#[test]
fn the_learner_s_giveaway_check_bands_no_giveaway() {
    for (entry, value, shown) in [
        (
            "giveaway_check = { upper = 0.60, lower = 0.20 }",
            0.55,
            false,
        ),
        (
            "giveaway_check = { upper = 0.40, lower = 0.20 }",
            0.45,
            true,
        ),
    ] {
        let config = format!("[cut_offs]\n{entry}\n");
        let mut defaults = prepared(None, [answer(CandidateCheck::NoGiveaway, value)]);
        let mut learners = prepared(Some(&config), [answer(CandidateCheck::NoGiveaway, value)]);

        assert_eq!(is_shown(&mut defaults), !shown, "{entry}: under 0.50");
        assert_eq!(
            is_shown(&mut learners),
            shown,
            "{entry}: under the learner's"
        );
        let giveaway = recorded_policy(&learners).cut_offs(Job::GiveawayCheck);
        assert_eq!(giveaway.origin(), CutOffsOrigin::Learner, "{entry}");
    }
}

#[test]
fn the_learner_s_question_check_no_longer_covers_no_giveaway() {
    let config = Some("[cut_offs]\nquestion_check = { upper = 0.60, lower = 0.20 }\n");

    let mut giveaway = prepared(config, [answer(CandidateCheck::NoGiveaway, 0.55)]);
    let mut needs_change = prepared(config, [answer(CandidateCheck::NeedsChange, 0.55)]);

    assert!(is_shown(&mut giveaway), "the giveaway check keeps 0.50");
    assert!(
        !is_shown(&mut needs_change),
        "unclear under the learner's 0.60"
    );
    let policy = recorded_policy(&giveaway);
    let giveaway = policy.cut_offs(Job::GiveawayCheck);
    assert_eq!(
        (
            giveaway.cut_offs(),
            giveaway.origin(),
            giveaway.policy().get()
        ),
        (cut_offs(0.50, 0.20), CutOffsOrigin::Default, 1)
    );
    let question_check = policy.cut_offs(Job::QuestionCheck);
    assert_eq!(
        (question_check.cut_offs(), question_check.origin()),
        (cut_offs(0.60, 0.20), CutOffsOrigin::Learner)
    );
}

#[test]
fn provenance_and_inspect_name_policy_1_and_the_giveaway_check_s_cut_offs() {
    let mut session = prepared(None, [answer(CandidateCheck::NoGiveaway, 0.55)]);
    assert!(
        session.text().contains(TESTED),
        "the tested question shows first: {}",
        session.text()
    );
    submit(&mut session, "a", "The key dedupes retries.");
    session.settle();

    let log = session.store().log();
    let requests: Vec<_> = log
        .iter()
        .flat_map(|entry| match entry {
            StoreEntry::Started { record, .. } => record.requests.iter().collect(),
            StoreEntry::Request { request, .. } => vec![request],
            _ => Vec::new(),
        })
        .collect();
    assert!(
        requests.len() >= 6,
        "the gate, writing, four checks and more"
    );
    for request in requests {
        assert_eq!(
            request.provenance.versions().policy().version().get(),
            1,
            "{:?}",
            request.asking
        );
    }
    activate(&mut session, "Inspect Jev's answers");
    let check = session.section("Question check");
    assert!(check[0].ends_with("· assessment policy 1"), "{check:#?}");
    for line in [
        "Cut-offs for the question check · upper 0.8, lower 0.2 · policy 1's",
        "Cut-offs for the giveaway check · upper 0.5, lower 0.2 · policy 1's",
        "The stem and options don't give the answer away · 0.55 · clear yes",
    ] {
        assert!(check.contains(&line.to_owned()), "{line}: {check:#?}");
    }
}
