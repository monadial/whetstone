//! Review withheld questions, against a scripted AI: it opens from No
//! questions passed checks and from the palette where the shortfall shows,
//! shows each withheld candidate with the checks that withheld it and Jev's
//! bands, and saves each vote on a check as a labelled example in the
//! labels file, in the session's own state folder. Voting never changes an
//! outcome, and the votes never reach the AI or the log.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use whetstone_ai::{Asked, ScriptedAi};
use whetstone_core::{CandidateCheck, OptionLetter, WrittenCandidate, WrittenCitation};

use crate::answering::type_text;
use crate::connected_code::{rust_pay, rust_payments};
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, generate, stems, touched_path,
};
use crate::send_starts_practice::{REVIEW_ABOUT_THE_DOMAIN, press_send};
use crate::session::{NO_COLOR, Session, Setup};

/// The stems of the candidates the generator writes first, both withheld.
const FIRST: &str = "Domain question 1?";
const SECOND: &str = "Domain question 2?";

/// A note the learner writes with a vote.
const NOTE: &str = "The options read alike, so nothing gives it away.";

/// A scripted AI that writes two candidates about the domain area, both
/// withheld: Jev can't tell whether the first's options give the answer
/// away, or whether its option B, which isn't its answer key, is right; and
/// it says the second doesn't need this change. On a Retry it writes two
/// more, which pass.
fn withholding_two() -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok([FIRST, SECOND]
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .into()),
    );
    ai.on_check_passing_except(
        FIRST,
        [
            answer(CandidateCheck::NoGiveaway, 0.46),
            answer(CandidateCheck::Option(OptionLetter::B), 0.5),
        ],
    );
    ai.on_check_passing_except(SECOND, [answer(CandidateCheck::NeedsChange, 0.1)]);
    let retried = stems("Retried", 2);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(retried
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    for stem in &retried {
        ai.on_check_passing(stem);
    }
    ai
}

/// A session `width` by `height` on No questions passed checks, with
/// [`withholding_two`]'s candidates withheld.
fn none_passed(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    let mut session = generate(ai, &[DOMAIN], PAYMENT_LIFECYCLE, width, height);
    session.settle();
    assert!(
        session.text().contains("No questions passed checks"),
        "{}",
        session.text()
    );
    session
}

/// A session `width` by `height` reviewing [`withholding_two`]'s withheld
/// candidates, opened from No questions passed checks.
fn reviewing(width: u16, height: u16) -> (Session, Arc<ScriptedAi>) {
    let ai = withholding_two();
    let mut session = none_passed(&ai, width, height);
    activate(&mut session, "Review withheld questions");
    (session, ai)
}

/// Votes Should have passed on the check at `number`, from 1, with `note`,
/// and lets the job that saves it end.
fn vote_should_have_passed(session: &mut Session, number: usize, note: &str) {
    activate(session, &format!("Vote on check {number}"));
    session.press("<Tab> <Enter> <Tab>");
    type_text(session, note);
    session.press("<Tab> <Enter>");
    session.settle();
}

/// Votes Rightly withheld on the check at `number`, from 1, with no note,
/// and lets the job that saves it end.
fn vote_rightly_withheld(session: &mut Session, number: usize) {
    activate(session, &format!("Vote on check {number}"));
    session.press("<Enter> <Tab> <Tab> <Tab> <Enter>");
    session.settle();
}

/// Skips the check at `number`, from 1.
fn skip(session: &mut Session, number: usize) {
    activate(session, &format!("Vote on check {number}"));
    session.press("<Tab> <Tab> <Tab> <Tab> <Enter>");
}

/// The labels file in the session's state folder.
fn labels_file(session: &Session) -> PathBuf {
    session.state_dir().join("labels.toml")
}

/// The labels file's candidate examples, as TOML tables.
fn examples(session: &Session) -> Vec<toml::Table> {
    let text = fs::read_to_string(labels_file(session)).expect("the labels file is written");
    let file: toml::Table = toml::from_str(&text).expect("the labels file is TOML");
    file["candidate"]
        .as_array()
        .expect("candidate examples")
        .iter()
        .map(|example| example.as_table().expect("a table").clone())
        .collect()
}

/// The rows under the heading "Checks that withheld it", trimmed, up to the
/// next blank row.
fn checks(session: &Session) -> Vec<String> {
    session.question().section("Checks that withheld it")
}

#[test]
fn review_withheld_questions_is_offered_on_no_questions_passed_checks() {
    let ai = withholding_two();
    let mut session = none_passed(&ai, 80, 24);

    let controls: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .filter(|row| {
            [
                "> Retry",
                "Pick another change",
                "Review withheld questions",
            ]
            .contains(&row.as_str())
        })
        .collect();
    assert_eq!(
        controls,
        [
            "> Retry",
            "Pick another change",
            "Review withheld questions"
        ]
    );

    activate(&mut session, "Review withheld questions");
    let text = session.text();
    assert!(text.contains("Withheld: not shown in Practice"), "{text}");
    assert_eq!(
        session.question().row_with("Withheld question"),
        "Withheld question 1 of 2 · Domain · Model the payment lifecycle"
    );
}

#[test]
fn it_shows_one_withheld_candidate_at_a_time_with_every_part_and_each_check_with_its_band() {
    let (mut session, _) = reviewing(120, 80);

    let text = session.text();
    for part in [
        FIRST,
        "A  Option A of the question",
        "B  Option B of the question",
        "C  Option C of the question",
        "Answer key: A",
        "The rationale for A, which stays hidden",
        "The rationale for C, which stays hidden",
        "Why it matters, which stays hidden",
        "Rubric, criterion 1 of 1",
        "The rubric's criterion, which stays hidden",
        "Met: What meeting it looks like",
        "Not met: What not meeting it looks like",
        "The hint, which stays hidden",
    ] {
        assert!(text.contains(part), "{part:?} is missing:\n{text}");
    }
    assert_eq!(
        session.question().section("Cited"),
        [format!("{}, line 1", touched_path(PAYMENT_LIFECYCLE))]
    );
    assert!(!text.contains(SECOND), "one at a time:\n{text}");
    assert_eq!(
        checks(&session),
        [
            "Jev's probability, not a measure of correctness",
            "1. The stem and options don't give the answer away",
            "Jev: 0.46 · unclear · it needed clear yes",
            "Your vote: Not voted yet",
            "2. Option B answers the question correctly",
            "Jev: 0.5 · unclear · it needed clear no",
            "Your vote: Not voted yet",
        ]
    );

    activate(&mut session, "Next withheld question");
    assert_eq!(
        session.question().row_with("Withheld question"),
        "Withheld question 2 of 2 · Domain · Model the payment lifecycle"
    );
    assert!(session.text().contains(SECOND));
    assert_eq!(
        checks(&session),
        [
            "Jev's probability, not a measure of correctness",
            "1. Needs this change, not general knowledge",
            "Jev: 0.1 · clear no · it needed clear yes",
            "Your vote: Not voted yet",
        ]
    );

    activate(&mut session, "Next withheld question");
    assert!(session.text().contains(FIRST), "it comes round");
}

#[test]
fn the_evidence_can_be_inspected_from_the_review() {
    let (mut session, _) = reviewing(80, 24);

    activate(&mut session, "Inspect source");

    assert_eq!(
        session.question().rows()[1].trim(),
        "Evidence: Model the payment lifecycle"
    );
    session.press("q");
    assert!(session.text().contains("Withheld: not shown in Practice"));
}

#[test]
fn the_review_says_where_votes_are_kept_and_that_they_hold_private_code_on_the_learner_s_repository()
 {
    let (session, _) = reviewing(120, 80);

    let text = session
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        text.contains(
            "Your votes are kept in ~/.local/state/whetstone/labels.toml, on this machine only, \
             and are never sent anywhere. On your own repository, they hold private code from \
             it. Voting never changes an outcome, and a withheld question never comes into \
             Practice."
        ),
        "{text}"
    );
}

#[test]
fn a_vote_a_skip_and_a_note_are_written_to_the_labels_file_created_owner_only() {
    let (mut session, _) = reviewing(120, 40);

    activate(&mut session, "Vote on check 1");
    session.press("<Tab> <Enter> <Tab>");
    type_text(&mut session, NOTE);
    session.press("<Tab> <Enter>");
    assert_eq!(
        checks(&session)[3],
        "Your vote: Should have passed · saving",
        "until the job that saves it ends"
    );
    session.settle();
    skip(&mut session, 2);

    assert_eq!(checks(&session)[3], "Your vote: Should have passed · saved");
    assert_eq!(checks(&session)[6], "Your vote: Skipped");
    let path = labels_file(&session);
    let mode = fs::metadata(&path)
        .expect("the labels file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "owner-only: {mode:o}");
    let [example] = examples(&session)
        .try_into()
        .expect("one vote, and no skip");
    assert_eq!(example["source"].as_str(), Some("learner vote"));
    assert_eq!(example["by"].as_str(), Some("learner"));
    assert_eq!(
        example["on"].as_datetime().map(ToString::to_string),
        Some("2026-10-21".to_owned())
    );
    assert_eq!(
        example["commit"].as_str(),
        Some("Model the payment lifecycle")
    );
    assert!(!example.contains_key("commit_id"), "a showcase change");
    assert_eq!(example["stem"].as_str(), Some(FIRST));
    assert_eq!(example["note"].as_str(), Some(NOTE));
    assert_eq!(
        example["labels"].as_table().map(ToString::to_string),
        Some("no_giveaway = \"yes\"\n".to_owned()),
        "Should have passed is the answer the show rule needed, and the skip has no label"
    );
    insta::assert_snapshot!(
        "labels_file",
        fs::read_to_string(&path).expect("the labels file")
    );
}

#[test]
fn a_vote_on_the_learner_s_repository_names_its_commit_by_its_full_id() {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    let commit = repo.commit("Double the charge");
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![WrittenCandidate {
            citations: vec![WrittenCitation {
                path: "src/pay.rs".to_owned(),
                first_line: 1,
                last_line: 1,
            }],
            ..candidate(FIRST, "A", PAYMENT_LIFECYCLE)
        }]),
    );
    ai.on_check_passing_except(FIRST, [answer(CandidateCheck::NoGiveaway, 0.46)]);
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            start: Some(repo.root()),
            ai: Some(&ai),
            ..Setup::default()
        },
        120,
        40,
    );
    session.press(REVIEW_ABOUT_THE_DOMAIN);
    session.settle();
    press_send(&mut session);
    session.settle();
    activate(&mut session, "Review withheld questions");

    vote_rightly_withheld(&mut session, 1);

    let [example] = examples(&session).try_into().expect("one vote");
    assert_eq!(example["commit"].as_str(), Some("Double the charge"));
    assert_eq!(example["commit_id"].as_str(), Some(commit.as_str()));
    assert_eq!(example["labels"].to_string(), "{ no_giveaway = \"no\" }");
}

#[test]
fn the_review_offers_the_check_that_a_criterion_asks_only_what_the_question_needs() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![candidate(FIRST, "A", PAYMENT_LIFECYCLE)]),
    );
    // Its one criterion asks for more than the question needs, so none is
    // left.
    ai.on_check_passing_except(FIRST, [answer(CandidateCheck::CriterionMinimum(0), 0.1)]);
    let mut session = none_passed(&ai, 120, 40);
    activate(&mut session, "Review withheld questions");

    assert_eq!(
        checks(&session),
        [
            "Jev's probability, not a measure of correctness",
            "1. Criterion 1 asks only what the question needs",
            "Jev: 0.1 · clear no · it needed clear yes",
            "Your vote: Not voted yet",
        ]
    );
    vote_rightly_withheld(&mut session, 1);

    let [example] = examples(&session).try_into().expect("one vote");
    assert_eq!(
        example["labels"].to_string(),
        "{ criterion_1_minimum = \"no\" }"
    );
}

#[test]
fn rightly_withheld_labels_the_answer_that_withheld_the_candidate_and_votes_add_up() {
    let (mut session, _) = reviewing(120, 40);

    vote_rightly_withheld(&mut session, 2);
    activate(&mut session, "Next withheld question");
    vote_rightly_withheld(&mut session, 1);

    let examples = examples(&session);
    let labels: Vec<(String, String)> = examples
        .iter()
        .map(|example| {
            (
                example["id"].as_str().unwrap_or_default().to_owned(),
                example["labels"].to_string(),
            )
        })
        .collect();
    assert_eq!(
        labels,
        [
            (
                "learner-vote-20261021T101502-1".to_owned(),
                "{ option_b = \"yes\" }".to_owned()
            ),
            (
                "learner-vote-20261021T101502-2".to_owned(),
                "{ needs_change = \"no\" }".to_owned()
            ),
        ],
        "option B, which isn't the answer key, needed clear no"
    );
    assert!(!examples[1].contains_key("note"));
}

#[test]
fn a_vote_saved_is_final_and_shows_when_the_review_opens_again() {
    let (mut session, _) = reviewing(120, 40);
    vote_rightly_withheld(&mut session, 1);

    session.press("q");
    activate(&mut session, "Review withheld questions");

    assert_eq!(checks(&session)[3], "Your vote: Rightly withheld · saved");
    assert_eq!(
        session.question().row_with("Vote on check 1"),
        "> Vote on check 1 (voted)    Vote on check 2"
    );
    session.press("<Enter>");
    assert!(
        !session.text().contains("Save vote"),
        "a vote saved is final"
    );
    assert_eq!(examples(&session).len(), 1);
}

#[test]
fn closing_the_vote_votes_nothing() {
    let (mut session, _) = reviewing(120, 40);

    activate(&mut session, "Vote on check 1");
    assert!(session.text().contains("Vote on check 1 "), "the box opens");
    session.press("<Tab> <Enter> <Tab>");
    type_text(&mut session, "Not sure");
    session.press("<Esc>");

    assert_eq!(checks(&session)[3], "Your vote: Not voted yet");
    assert!(!labels_file(&session).exists());
}

#[test]
fn voting_never_changes_an_outcome_or_brings_a_question_into_practice() {
    let ai = withholding_two();
    let mut session = none_passed(&ai, 120, 40);
    activate(&mut session, "Review withheld questions");
    vote_should_have_passed(&mut session, 1, NOTE);
    vote_should_have_passed(&mut session, 2, NOTE);
    activate(&mut session, "Next withheld question");
    vote_should_have_passed(&mut session, 1, NOTE);

    session.press("q");

    let text = session.text();
    assert!(text.contains("No questions passed checks"), "{text}");
    assert!(!text.contains(FIRST), "{text}");
    activate(&mut session, "Retry");
    session.settle();
    assert_eq!(
        session.question().row_with("Question 1 of"),
        "Question 1 of 2 · Domain"
    );
    for stem in [FIRST, SECOND] {
        assert!(!session.text().contains(stem), "{}", session.text());
    }
}

#[test]
fn the_votes_never_reach_any_ai_request() {
    let ai = withholding_two();
    let mut session = none_passed(&ai, 120, 40);
    let asked_before = ai.asked().len();
    activate(&mut session, "Review withheld questions");
    vote_should_have_passed(&mut session, 1, NOTE);
    assert_eq!(ai.asked().len(), asked_before, "voting asks nothing");

    session.press("q");
    activate(&mut session, "Retry");
    session.settle();

    let asked = ai.asked();
    assert!(asked.len() > asked_before, "Retry asks the AI again");
    for asked in &asked[asked_before..] {
        let (evidence, texts) = match asked {
            Asked::WriteCandidates { evidence, area, .. } => {
                (evidence, vec![area.prompt().to_owned()])
            }
            Asked::CheckQuestion {
                evidence,
                candidate,
            } => (
                evidence,
                vec![
                    candidate.stem().to_owned(),
                    candidate.why_it_matters().to_owned(),
                ],
            ),
            Asked::CheckAreas { .. }
            | Asked::AssessReasoning { .. }
            | Asked::Explain { .. }
            | Asked::CheckExplanation { .. } => panic!("Retry asks only to write and check"),
        };
        let mut texts = texts;
        texts.push(evidence.message().to_owned());
        texts.extend(evidence.areas().iter().map(|area| area.prompt().to_owned()));
        for text in texts {
            assert!(
                !text.contains(NOTE),
                "a vote's note reached the AI: {asked:?}"
            );
        }
    }
}

#[test]
fn a_vote_never_goes_to_the_log() {
    let ai = withholding_two();
    let (session, log) = capture(|| {
        let mut session = none_passed(&ai, 120, 40);
        activate(&mut session, "Review withheld questions");
        vote_should_have_passed(&mut session, 1, NOTE);
        session
    });

    assert!(log.contains("saved a vote to the labels file"), "{log}");
    for text in [NOTE, FIRST, "Should have passed", "no_giveaway"] {
        assert!(!log.contains(text), "{text:?} was logged:\n{log}");
    }
    assert_eq!(examples(&session).len(), 1);
}

/// Runs `run` with this thread's log captured, as every job runs on the
/// test's thread, and returns what it returned with the log.
fn capture<T>(run: impl FnOnce() -> T) -> (T, String) {
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
    // While only one dispatcher is alive, tracing asks only the calling
    // thread's whether a log line is wanted, and remembers the answer, so
    // other tests' threads, which have none, could make it drop this one's
    // lines. A second, which wants nothing, keeps every thread asking all.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let returned = tracing::subscriber::with_default(subscriber, run);
    let bytes = captured.0.lock().expect("log lock").clone();
    (
        returned,
        String::from_utf8(bytes).expect("the log is UTF-8"),
    )
}

#[test]
fn review_withheld_questions_is_offered_in_the_palette_where_the_shortfall_shows() {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 3);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    ai.on_check_passing(&stems[0]);
    ai.on_check_passing(&stems[1]);
    ai.on_check_passing_except(&stems[2], [answer(CandidateCheck::NoGiveaway, 0.46)]);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 80, 24);
    session.settle();

    assert_eq!(
        session.question().row_with("passed checks"),
        "2 passed checks, 4 asked for · see why in actions"
    );
    session.press(":");
    let actions: Vec<String> = session
        .overlay_rows()
        .iter()
        .filter_map(|row| row.split("  ").next())
        .map(|action| action.trim_start_matches("> ").to_owned())
        .skip_while(|action| action != "Inspect source")
        .take(3)
        .collect();
    assert_eq!(
        actions,
        [
            "Inspect source",
            "See why fewer passed",
            "Review withheld questions"
        ],
        "beside See why fewer passed"
    );
    session.press("j j <Enter>");

    assert_eq!(
        session.question().row_with("Withheld question"),
        "Withheld question 1 of 1 · Domain · Model the payment lifecycle"
    );
    assert!(session.text().contains(&stems[2]));
    session.press("q");
    assert_eq!(
        session.question().row_with("Question 1 of"),
        "Question 1 of 2 · Domain"
    );
}

#[test]
fn the_review_at_120x32() {
    let (session, _) = reviewing(120, 32);

    insta::assert_debug_snapshot!("review_withheld_120x32", session.frame());
}

#[test]
fn the_review_at_80x24() {
    let (session, _) = reviewing(80, 24);

    insta::assert_debug_snapshot!("review_withheld_80x24", session.frame());
}

#[test]
fn the_vote_at_120x32() {
    let (mut session, _) = reviewing(120, 32);
    activate(&mut session, "Vote on check 2");

    insta::assert_debug_snapshot!("vote_120x32", session.frame());
}

#[test]
fn the_vote_at_80x24() {
    let (mut session, _) = reviewing(80, 24);
    activate(&mut session, "Vote on check 2");

    insta::assert_debug_snapshot!("vote_80x24", session.frame());
}
