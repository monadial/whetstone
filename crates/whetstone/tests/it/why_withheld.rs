//! Why fewer questions passed their checks than were asked for, or none:
//! No questions passed checks counts the candidates written and withheld,
//! gives each check that withheld them in words, telling "Jev said no"
//! from "Jev couldn't tell", counts the plain checks' drops apart, and says
//! whose cut-offs decided. The shortfall line offers the same reasons
//! behind an action. No probability or cut-off value shows.

use std::sync::Arc;

use whetstone_ai::ScriptedAi;
use whetstone_core::{
    CandidateCheck, Depth, OptionLetter, Probability, QuestionCheckAnswers, WrittenCandidate,
    WrittenCitation,
};

use crate::answering::submit;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, generate, generate_in, reasoning_met,
    stems, touched_path, writing_about_the_domain,
};
use crate::session::{NO_COLOR, Session, Setup};

/// A scripted AI whose generator writes one candidate about the domain
/// area of the payment lifecycle change for each of `stems`, in order.
fn writing(stems: &[&str]) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    ai
}

/// A scripted AI whose generator writes one candidate for each stem of
/// `checked`, in order, and whose Jev passes each but for its answers.
fn checking(checked: &[(&str, Vec<(CandidateCheck, Probability)>)]) -> Arc<ScriptedAi> {
    let ai = writing(&checked.iter().map(|(stem, _)| *stem).collect::<Vec<_>>());
    for (stem, answers) in checked {
        ai.on_check_passing_except(stem, answers.clone());
    }
    ai
}

/// A session, `width` by `height`, that prepared the questions `ai`
/// writes and checks about the domain area of the payment lifecycle
/// change, with every result delivered.
fn prepared(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    let mut session = generate(ai, &[DOMAIN], PAYMENT_LIFECYCLE, width, height);
    session.settle();
    session
}

fn probability(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// The rows between the subject and Retry writes and checks new ones,
/// trimmed: the reasons.
fn reasons(session: &Session) -> Vec<String> {
    let rows: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .collect();
    let first = rows
        .iter()
        .position(|row| row.contains(" written"))
        .expect("the count of candidates written");
    rows[first..]
        .iter()
        .take_while(|row| !row.is_empty())
        .cloned()
        .collect()
}

#[test]
fn the_reasons_tell_jev_saying_no_from_jev_not_being_able_to_tell_across_checks() {
    let ai = checking(&[
        (
            "First?",
            vec![
                answer(CandidateCheck::NoGiveaway, 0.5),
                answer(CandidateCheck::NeedsChange, 0.6),
            ],
        ),
        (
            "Second?",
            vec![
                answer(CandidateCheck::NoGiveaway, 0.45),
                answer(CandidateCheck::NeedsChange, 0.1),
            ],
        ),
        ("Third?", vec![answer(CandidateCheck::NoGiveaway, 0.15)]),
    ]);

    let session = prepared(&ai, 120, 32);

    assert_eq!(
        reasons(&session),
        [
            "3 written · 3 withheld, using policy 0's cut-offs",
            "2: Jev couldn't tell whether the options give the answer away",
            "1: Jev said the question doesn't need this change",
            "1: Jev couldn't tell whether the question needs this change",
            "1: Jev said the options give the answer away",
        ],
        "{}",
        session.text()
    );
}

#[test]
fn each_check_that_withholds_a_candidate_has_one_sentence_for_jev_saying_no_and_one_for_not_telling()
 {
    use CandidateCheck::*;

    let failing = [
        (Answerable, 0.1),
        (Answerable, 0.5),
        (AboutArea, 0.1),
        (AboutArea, 0.5),
        (NeedsChange, 0.1),
        (NeedsChange, 0.5),
        (NoGiveaway, 0.1),
        (NoGiveaway, 0.5),
        (Criterion(0), 0.1),
        (Criterion(0), 0.5),
        // The only criterion asks for more than the question needs, so none
        // is left.
        (CriterionMinimum(0), 0.1),
        (CriterionMinimum(0), 0.5),
        // The answer key is A: Jev clearly says no to it, or can't tell.
        (Option(OptionLetter::A), 0.1),
        (Option(OptionLetter::A), 0.5),
        // B isn't the answer key: Jev clearly says yes to it, or can't tell.
        (Option(OptionLetter::B), 0.9),
        (Option(OptionLetter::B), 0.5),
        (Rationale(OptionLetter::C), 0.1),
        (Rationale(OptionLetter::C), 0.5),
        (WhyItMatters, 0.1),
        (WhyItMatters, 0.5),
    ];
    let stems: Vec<String> = (1..=failing.len())
        .map(|number| format!("Failing {number}?"))
        .collect();
    let mut written: Vec<&str> = stems.iter().map(String::as_str).collect();
    written.push("Unanswered?");
    let ai = writing(&written);
    for (stem, (check, value)) in stems.iter().zip(failing) {
        ai.on_check_passing_except(stem, [answer(check, value)]);
    }
    let checks = [
        Answerable,
        AboutArea,
        NeedsChange,
        Criterion(0),
        CriterionMinimum(0),
        Option(OptionLetter::A),
        Option(OptionLetter::B),
        Option(OptionLetter::C),
        Rationale(OptionLetter::A),
        Rationale(OptionLetter::B),
        Rationale(OptionLetter::C),
        WhyItMatters,
        HintPointsToEvidence,
        HintHidesAnswerKey,
    ];
    let answers = checks.map(|check| match check {
        Option(OptionLetter::B | OptionLetter::C) => answer(check, 0.1),
        _ => answer(check, 0.9),
    });
    let depth = Depth::new([0.2, 0.3, 0.5].map(probability), 1.3).expect("from 0 to 2");
    // Jev leaves the giveaway check unanswered.
    ai.on_check("Unanswered?", Ok(QuestionCheckAnswers::new(answers, depth)));

    let session = prepared(&ai, 120, 60);

    assert_eq!(
        reasons(&session),
        [
            "21 written · 21 withheld, using policy 0's cut-offs",
            "1: Jev said the question can't be answered from the evidence",
            "1: Jev couldn't tell whether the question can be answered from the evidence",
            "1: Jev said the question isn't about its area",
            "1: Jev couldn't tell whether the question is about its area",
            "1: Jev said the question doesn't need this change",
            "1: Jev couldn't tell whether the question needs this change",
            "1: Jev said the options give the answer away",
            "1: Jev couldn't tell whether the options give the answer away",
            "1: Jev didn't answer whether the options give the answer away",
            "1: Jev said a rubric criterion doesn't follow from the evidence, or can't be judged \
             from written reasoning",
            "1: Jev couldn't tell whether a rubric criterion follows from the evidence and can be \
             judged from written reasoning",
            "1: Jev said every rubric criterion asks for more than the question needs",
            "1: Jev couldn't tell whether any rubric criterion asks only for what the question needs",
            "1: Jev said the answer key doesn't answer the question",
            "1: Jev couldn't tell whether the answer key answers the question",
            "1: Jev said an option besides the answer key answers the question",
            "1: Jev couldn't tell whether an option besides the answer key answers the question",
            "1: Jev said an option's rationale isn't supported by the evidence",
            "1: Jev couldn't tell whether an option's rationale is supported by the evidence",
            "1: Jev said \"why it matters\" isn't supported by the evidence",
            "1: Jev couldn't tell whether \"why it matters\" is supported by the evidence",
        ],
        "{}",
        session.text()
    );
}

#[test]
fn a_candidate_with_no_criterion_left_gives_one_reason_as_unclear_as_its_least_clear_answer() {
    use CandidateCheck::*;

    let mut two_criteria = candidate("Two criteria?", "A", PAYMENT_LIFECYCLE);
    two_criteria.rubric.push(two_criteria.rubric[0].clone());
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(&area_id(DOMAIN), Ok(vec![two_criteria]));
    ai.on_check_passing_except(
        "Two criteria?",
        [
            answer(CriterionMinimum(0), 0.1),
            answer(CriterionMinimum(1), 0.5),
        ],
    );

    let session = prepared(&ai, 120, 32);

    assert_eq!(
        reasons(&session),
        [
            "1 written · 1 withheld, using policy 0's cut-offs",
            "1: Jev couldn't tell whether any rubric criterion asks only for what the question needs",
        ],
        "{}",
        session.text()
    );
}

/// A candidate for `stem` the plain checks drop, made by `malform`.
fn malformed(stem: &str, malform: impl FnOnce(&mut WrittenCandidate)) -> WrittenCandidate {
    let mut written = candidate(stem, "A", PAYMENT_LIFECYCLE);
    malform(&mut written);
    written
}

#[test]
fn the_plain_checks_drops_are_counted_apart_from_the_candidates_withheld() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![
            candidate("Withheld first?", "A", PAYMENT_LIFECYCLE),
            malformed("Two options?", |written| {
                written.options.pop();
            }),
            malformed("Cites a file not sent?", |written| {
                written.citations = vec![WrittenCitation {
                    path: "src/not_in_the_change.rs".to_owned(),
                    first_line: 1,
                    last_line: 1,
                }];
            }),
            malformed("Also two options?", |written| {
                written.options.pop();
            }),
            malformed("No hint?", |written| written.hint.clear()),
            candidate("Withheld second?", "A", PAYMENT_LIFECYCLE),
        ]),
    );
    for stem in ["Withheld first?", "Withheld second?"] {
        ai.on_check_passing_except(stem, [answer(CandidateCheck::NoGiveaway, 0.5)]);
    }

    let session = prepared(&ai, 120, 32);

    assert_eq!(
        reasons(&session),
        [
            "6 written · 2 withheld, using policy 0's cut-offs",
            "all 2: Jev couldn't tell whether the options give the answer away",
            "2 dropped: each had options other than exactly A, B and C",
            "1 dropped: it cited lines that aren't in the evidence",
            "1 dropped: it was missing its hint",
        ],
        "{}",
        session.text()
    );
}

#[test]
fn with_every_candidate_dropped_no_cut_offs_decided_anything() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![
            malformed("Cites nothing?", |written| written.citations.clear()),
            malformed("Answer key D?", |written| {
                "D".clone_into(&mut written.answer_key);
            }),
        ]),
    );

    let session = prepared(&ai, 120, 32);

    assert_eq!(
        reasons(&session),
        [
            "2 written",
            "1 dropped: it was missing a citation",
            "1 dropped: it had an answer key that isn't one of its options",
        ],
        "{}",
        session.text()
    );
}

#[test]
fn a_hint_that_failed_its_checks_is_never_a_reason() {
    let ai = checking(&[(
        "Withheld?",
        vec![
            answer(CandidateCheck::NeedsChange, 0.5),
            answer(CandidateCheck::HintPointsToEvidence, 0.1),
            answer(CandidateCheck::HintHidesAnswerKey, 0.5),
        ],
    )]);

    let session = prepared(&ai, 120, 32);

    assert_eq!(
        reasons(&session),
        [
            "1 written · 1 withheld, using policy 0's cut-offs",
            "1: Jev couldn't tell whether the question needs this change",
        ],
        "{}",
        session.text()
    );
    assert!(!session.text().contains("hint"), "{}", session.text());
}

/// The reasons when `ai` prepared the questions in a session whose global
/// config is `config`.
fn reasons_under(config: &str, ai: &Arc<ScriptedAi>) -> Vec<String> {
    let mut session = generate_in(
        Setup {
            config: Some(config),
            variables: NO_COLOR,
            ai: Some(ai),
            ..Setup::default()
        },
        &[DOMAIN],
        PAYMENT_LIFECYCLE,
        120,
        32,
    );
    session.settle();
    reasons(&session)
}

#[test]
fn the_reasons_say_when_the_learner_s_cut_offs_from_config_decided() {
    let ai = checking(&[("Withheld?", vec![answer(CandidateCheck::NoGiveaway, 0.82)])]);

    let reasons = reasons_under(
        "[cut_offs]\nquestion_check = { upper = 0.85, lower = 0.20 }\n",
        &ai,
    );

    assert_eq!(
        reasons,
        [
            "1 written · 1 withheld, using your cut-offs from config",
            "1: Jev couldn't tell whether the options give the answer away",
        ]
    );
}

#[test]
fn the_reasons_say_policy_0_s_cut_offs_decided_when_config_changes_only_other_jobs() {
    let ai = checking(&[("Withheld?", vec![answer(CandidateCheck::NoGiveaway, 0.5)])]);

    let reasons = reasons_under(
        "[cut_offs]\noption_rating = { upper = 0.70, lower = 0.30 }\n",
        &ai,
    );

    assert_eq!(
        reasons[0],
        "1 written · 1 withheld, using policy 0's cut-offs"
    );
}

#[test]
fn the_reasons_say_when_the_learner_s_cut_offs_and_policy_0_s_both_decided() {
    let ai = checking(&[
        ("Giveaway?", vec![answer(CandidateCheck::NoGiveaway, 0.82)]),
        (
            "Option B?",
            vec![answer(CandidateCheck::Option(OptionLetter::B), 0.5)],
        ),
    ]);

    let reasons = reasons_under(
        "[cut_offs]\nquestion_check = { upper = 0.85, lower = 0.20 }\n",
        &ai,
    );

    assert_eq!(
        reasons[0],
        "2 written · 2 withheld, using your cut-offs from config and policy 0's"
    );
}

/// A scripted AI as the showcase goes live today: the generator writes six
/// candidates, one citing lines the evidence doesn't send, and Jev can't
/// tell whether the options of the other five give the answer away, nor,
/// for three of them, whether they need this change.
fn as_live() -> Arc<ScriptedAi> {
    let stems = stems("Domain", 5);
    let mut written: Vec<WrittenCandidate> = stems
        .iter()
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written.insert(
        2,
        malformed("Cites lines past the end?", |written| {
            written.citations = vec![WrittenCitation {
                path: touched_path(PAYMENT_LIFECYCLE),
                first_line: 1,
                last_line: 100_000,
            }];
        }),
    );
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(&area_id(DOMAIN), Ok(written));
    for (place, stem) in stems.iter().enumerate() {
        let mut answers = vec![answer(CandidateCheck::NoGiveaway, 0.4)];
        if place < 3 {
            answers.push(answer(CandidateCheck::NeedsChange, 0.7));
        }
        ai.on_check_passing_except(stem, answers);
    }
    ai
}

#[test]
fn no_questions_passed_checks_gives_its_reasons_in_words() {
    let session = prepared(&as_live(), 120, 32);

    assert_eq!(
        reasons(&session),
        [
            "6 written · 5 withheld, using policy 0's cut-offs",
            "all 5: Jev couldn't tell whether the options give the answer away",
            "3: Jev couldn't tell whether the question needs this change",
            "1 dropped: it cited lines that aren't in the evidence",
        ],
        "{}",
        session.text()
    );
    let text = session.text();
    for number in ["0.4", "0.7", "0.8", "0.2"] {
        assert!(!text.contains(number), "no probability or cut-off: {text}");
    }
}

#[test]
fn no_questions_passed_checks_with_its_reasons_at_120x32() {
    let session = prepared(&as_live(), 120, 32);

    insta::assert_debug_snapshot!(
        "no_questions_passed_checks_with_reasons_120x32",
        session.frame()
    );
}

#[test]
fn no_questions_passed_checks_with_its_reasons_at_80x24() {
    let session = prepared(&as_live(), 80, 24);

    insta::assert_debug_snapshot!(
        "no_questions_passed_checks_with_reasons_80x24",
        session.frame()
    );
}

#[test]
fn after_retry_the_reasons_are_the_new_candidates_alone() {
    let ai = Arc::new(ScriptedAi::new());
    let first = stems("First try", 2);
    let second = stems("Second try", 3);
    for stems in [&first, &second] {
        ai.on_write(
            &area_id(DOMAIN),
            Ok(stems
                .iter()
                .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
                .collect()),
        );
    }
    for stem in &first {
        ai.on_check_passing_except(stem, [answer(CandidateCheck::Answerable, 0.1)]);
    }
    for stem in &second {
        ai.on_check_passing_except(stem, [answer(CandidateCheck::AboutArea, 0.5)]);
    }
    let mut session = prepared(&ai, 120, 32);

    session.press("<Enter>");
    session.settle();

    assert_eq!(
        reasons(&session),
        [
            "3 written · 3 withheld, using policy 0's cut-offs",
            "all 3: Jev couldn't tell whether the question is about its area",
        ],
        "{}",
        session.text()
    );
}

/// A session where 3 of the 4 questions asked for passed: the generator
/// wrote six, one of which the plain checks dropped, and Jev couldn't tell
/// whether the options of two others give the answer away. Reasoning
/// submitted to those shown meets its criterion.
fn a_shortfall(width: u16, height: u16) -> Session {
    let passing = stems("Passing", 3);
    let ai = Arc::new(ScriptedAi::new());
    let mut written: Vec<WrittenCandidate> = passing
        .iter()
        .chain(["Withheld first?".to_owned(), "Withheld second?".to_owned()].iter())
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written.push(malformed("No hint?", |written| written.hint.clear()));
    ai.on_write(&area_id(DOMAIN), Ok(written));
    for stem in &passing {
        ai.on_check_passing(stem);
        reasoning_met(&ai, stem);
    }
    for stem in ["Withheld first?", "Withheld second?"] {
        ai.on_check_passing_except(stem, [answer(CandidateCheck::NoGiveaway, 0.5)]);
    }
    prepared(&ai, width, height)
}

#[test]
fn the_shortfall_line_offers_why_fewer_passed() {
    let session = a_shortfall(80, 24);

    assert_eq!(
        session.question().row_with("passed checks"),
        "3 passed checks, 4 asked for · see why in actions"
    );
}

#[test]
fn see_why_fewer_passed_gives_the_same_reasons_and_back_returns_to_the_question() {
    let mut session = a_shortfall(80, 24);

    session.press(": j <Enter>");

    let text = session.text();
    assert!(text.contains("Why fewer passed checks"), "{text}");
    for reason in [
        "6 written · 3 passed · 2 withheld, using policy 0's cut-offs",
        "all 2: Jev couldn't tell whether the options give the answer away",
        "1 dropped: it was missing its hint",
    ] {
        assert!(text.contains(reason), "{reason}: {text}");
    }
    session.press("q");
    assert_eq!(
        session.question().row_with("Question 1"),
        "Question 1 of 3 · Domain"
    );
}

#[test]
fn see_why_fewer_passed_is_offered_on_feedback_too() {
    let mut session = a_shortfall(120, 32);
    submit(&mut session, "a", "Because.");

    session.press(": j <Enter>");

    assert!(
        session.text().contains("Why fewer passed checks"),
        "{}",
        session.text()
    );
}

#[test]
fn see_why_fewer_passed_is_offered_only_with_a_shortfall() {
    let ai = Arc::new(ScriptedAi::new());
    let passing = stems("Passing", 4);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(passing
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    for stem in &passing {
        ai.on_check_passing(stem);
    }
    let mut session = prepared(&ai, 120, 32);

    session.press(":");

    assert!(!session.text().contains("See why"), "{}", session.text());
}

#[test]
fn see_why_fewer_passed_at_120x32() {
    let mut session = a_shortfall(120, 32);
    session.press(": j <Enter>");

    insta::assert_debug_snapshot!("see_why_fewer_passed_120x32", session.frame());
}

#[test]
fn see_why_fewer_passed_at_80x24() {
    let mut session = a_shortfall(80, 24);
    session.press(": j <Enter>");

    insta::assert_debug_snapshot!("see_why_fewer_passed_80x24", session.frame());
}

#[test]
fn see_why_fewer_passed_says_when_the_generator_wrote_fewer_than_were_asked_for() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    let mut session = prepared(&ai, 80, 24);

    session.press(": j <Enter>");

    let text = session.text();
    assert!(text.contains("3 written · 3 passed"), "{text}");
    assert!(!text.contains("withheld"), "{text}");
}
