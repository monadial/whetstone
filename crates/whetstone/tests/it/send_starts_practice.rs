//! Send on the outbound review starts Practice on the learner's repository:
//! Preparing questions opens in the review's place, with the outbound
//! review ticked in its stage row, and every request to Jev and the
//! generator carries exactly the evidence the learner reviewed, and nothing
//! of a file they left out. Send needs an `OpenRouter` key, which it asks
//! for first, then sendable evidence, and waits while connected code is
//! found.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use whetstone_ai::{
    Asked, ParagraphCheck, QuestionCheckCall, RequestFailure, ScriptedAi, WrittenParagraph,
    candidates_request, question_check_request,
};
use whetstone_core::{
    ConnectionGroup, CriterionOutcome, Evidence, ModelId, Probability, WrittenCandidate,
    WrittenCitation,
};

use crate::answering::submit;
use crate::connected_code::{AWS_KEY, REVIEW_NEWEST, rust_pay, rust_payments, send};
use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{NO_COLOR, Session, Setup, columns};

/// From home: open the repository's picker, select only the domain area,
/// choose the newest commit, and open its outbound review.
pub(crate) const REVIEW_ABOUT_THE_DOMAIN: &str =
    "<Enter> j <Space> j <Space> j <Space> j <Space> j <Space> k k k k k <S-Tab> <Enter>";
/// From the change on the outbound review: down to `plan.txt`, the second
/// touched file, and leave it out.
const LEAVE_OUT_THE_PLAN: &str = "j j <Space>";
/// The touched file the learner leaves out, and a line only it holds.
const PLAN: &str = "plan.txt";
const PLAN_TEXT: &str = "Raise prices again next quarter.";
/// From anywhere on the outbound review, up to the change.
const TO_THE_CHANGE: &str = "k k k k k k k k k k k k k k k k k k k k";

/// `rust_payments` with a manifest and a test for `src/pay.rs`, and a
/// newest commit that doubles the charge in `src/pay.rs`, which has
/// connected code, adds `notes.txt`, which has none, and adds `plan.txt`,
/// which the learner leaves out.
fn doubling_the_charge() -> TestRepo {
    let repo = rust_payments();
    repo.write("Cargo.toml", "[package]\nname = \"shop\"\n")
        .write("tests/pay.rs", "#[test]\nfn charges() {}\n");
    repo.commit("Add the manifest and a test");
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"))
        .write("notes.txt", "Charge twice as much from now on.\n")
        .write(PLAN, format!("{PLAN_TEXT}\n"));
    repo.commit("Double the charge");
    repo
}

/// `rust_payments` with a newest commit that doubles the charge and adds
/// `config.txt`, which holds an AWS key.
fn with_a_secret() -> TestRepo {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"))
        .write("config.txt", format!("key = {AWS_KEY}\n"));
    repo.commit("Double the charge");
    repo
}

/// A start without colour in `repo`, whose Jev and generator are `ai`.
fn setup<'a>(repo: &'a TestRepo, ai: &'a Arc<ScriptedAi>) -> Setup<'a> {
    Setup {
        variables: NO_COLOR,
        start: Some(repo.root()),
        ai: Some(ai),
        ..Setup::default()
    }
}

/// A session `width` by `height`, started in `repo` with `ai`, on the
/// outbound review of its newest commit, reached with the keys `review`,
/// once connected code is found.
pub(crate) fn reviewing(
    repo: &TestRepo,
    ai: &Arc<ScriptedAi>,
    review: &str,
    width: u16,
    height: u16,
) -> Session {
    let mut session = Session::start(setup(repo, ai), width, height);
    session.press(review);
    session.settle();
    session
}

/// Presses Send on the outbound review, from wherever focus is: up to the
/// change, round to Back, then up to Send.
pub(crate) fn press_send(session: &mut Session) {
    session.press(TO_THE_CHANGE);
    session.press("<S-Tab> k");
    assert_eq!(session.row_with("> Send"), "> Send", "Send is enabled");
    session.press("<Enter>");
}

/// A scripted AI whose generator writes one candidate about the domain
/// area, citing the first line of `src/pay.rs`, which passes every check.
/// Once submitted, its reasoning misses its criterion, and the generator's
/// explanation of that, citing the same line, passes Jev's check.
fn writing_one_about_the_domain() -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stem = "Why does the charge double?";
    let first_line = || WrittenCitation {
        path: "src/pay.rs".to_owned(),
        first_line: 1,
        last_line: 1,
    };
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![WrittenCandidate {
            citations: vec![first_line()],
            ..candidate(stem, "A", PAYMENT_LIFECYCLE)
        }]),
    );
    ai.on_check_passing(stem);
    let probability = |value| Probability::new(value).expect("from 0 to 1");
    ai.on_assess(stem, Ok(vec![probability(0.1)]));
    ai.on_explain(
        stem,
        Ok(vec![WrittenParagraph {
            criterion: 0,
            outcome: CriterionOutcome::NotMet,
            text: "You didn't say the retry sends a new key.".to_owned(),
            citations: vec![first_line()],
        }]),
    );
    ai.on_check_explanation(
        stem,
        Ok(vec![ParagraphCheck {
            supported: probability(0.9),
            agrees: probability(0.9),
        }]),
    );
    ai
}

/// The review of `repo`, made by `doubling_the_charge`, about the domain
/// area, with `plan.txt` left out, `width` by `height`, after Send, once
/// every request `ai` was asked is answered.
fn sent_without_the_plan(
    repo: &TestRepo,
    ai: &Arc<ScriptedAi>,
    width: u16,
    height: u16,
) -> Session {
    let mut session = reviewing(repo, ai, REVIEW_ABOUT_THE_DOMAIN, width, height);
    session.press(LEAVE_OUT_THE_PLAN);
    press_send(&mut session);
    session.settle();
    session
}

/// What the learner saw of one touched file on the outbound review: its
/// row's path and status, and, if it is sent, the diff and the content its
/// reader shows.
#[derive(Debug, PartialEq, Eq)]
struct ShownFile {
    path: String,
    status: String,
    sent: Option<[Vec<String>; 2]>,
}

/// What the learner saw of one connection on the outbound review, connected
/// code or a related test or configuration: its row's path, extent and
/// status, and the lines its reader shows, with the first one's number.
#[derive(Debug, PartialEq, Eq)]
struct ShownConnection {
    path: String,
    extent: String,
    status: String,
    lines: (usize, Vec<String>),
}

/// What the learner saw of one area on the outbound review: its row's name
/// and prompt source, and its area prompt as its reader shows it, every run
/// of white space as one space.
#[derive(Debug, PartialEq, Eq)]
struct ShownArea {
    name: String,
    source: String,
    prompt: String,
}

/// What the learner saw on the outbound review: the message, each touched
/// file, each connection and each area, read whole, and the size.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    message: Vec<String>,
    files: Vec<ShownFile>,
    connections: Vec<ShownConnection>,
    areas: Vec<ShownArea>,
    size: String,
}

/// `text` with every run of white space as one space.
fn spaced(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The lines under `heading` on the reader, up to the next blank row: as
/// they are, or, for numbered code, without their numbers, with the first
/// one's number.
fn read_under(session: &Session, heading: &str) -> (usize, Vec<String>) {
    let rows: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim_end().to_owned())
        .collect();
    let start = rows.iter().position(|row| row.trim() == heading);
    assert!(start.is_some(), "no {heading:?}:\n{}", session.text());
    let mut first = None;
    let lines = rows
        .iter()
        .skip(start.unwrap_or_default() + 1)
        .take_while(|row| !row.trim().is_empty())
        .map(|row| {
            let row = row.trim_start();
            let (number, line) = row.split_once(' ').unwrap_or((row, ""));
            match number.parse::<usize>() {
                Ok(number) => {
                    first.get_or_insert(number);
                    line.to_owned()
                }
                Err(_) => row.to_owned(),
            }
        })
        .collect();
    (first.unwrap_or(1), lines)
}

/// The prose under `heading` on the reader, to the key hints, every run of
/// white space as one space, however it wraps.
fn read_prose_under(session: &Session, heading: &str) -> String {
    let rows = session.rows();
    let start = rows.iter().position(|row| row.trim() == heading);
    assert!(start.is_some(), "no {heading:?}:\n{}", session.text());
    let prose: Vec<&str> = rows
        .iter()
        .skip(start.unwrap_or_default() + 1)
        .map(|row| row.trim())
        .take_while(|row| !row.starts_with("? keys"))
        .collect();
    spaced(&prose.join(" "))
}

/// The rows of the section whose summary starts with `summary`, after it,
/// split into their columns, without the focus marker.
fn listed(session: &Session, summary: &str) -> Vec<Vec<String>> {
    let rows = session.rows();
    let start = rows.iter().position(|row| row.trim().starts_with(summary));
    assert!(start.is_some(), "no {summary:?}:\n{}", session.text());
    let rows: Vec<String> = rows
        .iter()
        .skip(start.unwrap_or_default() + 1)
        .map(|row| row.trim().to_owned())
        .take_while(|row| !row.is_empty())
        .collect();
    columns(&rows)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|column| column.trim_start_matches("> ").to_owned())
                .collect()
        })
        .collect()
}

/// Opens the reader of the outbound review's control `at` places after the
/// change, reads it with `read`, and goes back to the change.
fn reading<T>(session: &mut Session, at: usize, read: impl Fn(&Session) -> T) -> T {
    session.press(&"j ".repeat(at));
    session.press("<Enter>");
    let read = read(session);
    session.press("q");
    session.press(TO_THE_CHANGE);
    read
}

/// Everything the outbound review in `session` shows will be sent, read
/// from its rows and from each control's reader, with focus back on the
/// change.
fn shown(session: &mut Session) -> Shown {
    session.press(TO_THE_CHANGE);
    let file_rows = listed(session, "Files");
    let connection_rows = [
        listed(session, "Connected code"),
        listed(session, "Related tests and configuration"),
    ]
    .concat();
    let area_rows = listed(session, "Areas");
    let size = session.section("Size")[0].clone();
    let message = reading(session, 0, |session| {
        read_under(session, "Message, as it is sent").1
    });
    let files = file_rows
        .iter()
        .enumerate()
        .map(|(at, row)| ShownFile {
            path: row[0].clone(),
            status: row[row.len() - 1].clone(),
            sent: reading(session, at + 1, |session| {
                let text = session.text();
                let content = if text.contains("Full content before") {
                    "Full content before the change, which deletes it"
                } else {
                    "Full content"
                };
                text.contains("Diff").then(|| {
                    [
                        read_under(session, "Diff").1,
                        read_under(session, content).1,
                    ]
                })
            }),
        })
        .collect();
    let connections = connection_rows
        .iter()
        .enumerate()
        .map(|(at, row)| ShownConnection {
            path: row[0].clone(),
            extent: row[1].clone(),
            status: row[3].clone(),
            lines: reading(session, file_rows.len() + at + 1, |session| {
                read_under(session, "Content, as it is sent")
            }),
        })
        .collect();
    let areas = area_rows
        .iter()
        .enumerate()
        .map(|(at, row)| ShownArea {
            name: row[0].clone(),
            source: row[1].clone(),
            prompt: reading(
                session,
                file_rows.len() + connection_rows.len() + at + 1,
                |session| read_prose_under(session, "Area prompt, as it is sent"),
            ),
        })
        .collect();
    Shown {
        message,
        files,
        connections,
        areas,
        size,
    }
}

/// What `evidence` sends, in the terms and the order the outbound review
/// shows it in: connected code, then related tests and configuration.
fn as_shown(evidence: &Evidence) -> Shown {
    let lines = |text: &str| {
        text.lines()
            .map(|line| line.trim_end().to_owned())
            .collect()
    };
    let connected = |connection: &&_| {
        whetstone_core::EvidenceConnection::group(connection) == ConnectionGroup::ConnectedCode
    };
    let (connected_code, related): (Vec<_>, Vec<_>) =
        evidence.connections().iter().partition(connected);
    Shown {
        message: lines(evidence.message()),
        files: evidence
            .files()
            .iter()
            .map(|file| ShownFile {
                path: file.path().to_owned(),
                status: match file.connected() {
                    0 => file.status().to_string(),
                    connected => format!("{} · {connected} connected", file.status()),
                },
                sent: file
                    .sent()
                    .map(|sent| [lines(sent.diff), lines(sent.content)]),
            })
            .collect(),
        connections: connected_code
            .into_iter()
            .chain(related)
            .map(|connection| ShownConnection {
                path: connection.id().path().to_owned(),
                extent: connection.id().extent().to_string(),
                status: connection.status().to_string(),
                lines: connection
                    .sent()
                    .map(|sent| (sent.first_line, lines(sent.text)))
                    .unwrap_or_default(),
            })
            .collect(),
        areas: evidence
            .areas()
            .iter()
            .map(|area| ShownArea {
                name: area.name().to_owned(),
                source: area.source().to_string(),
                prompt: spaced(area.prompt()),
            })
            .collect(),
        size: format!(
            "{} KB of {} KB",
            evidence.size().div_ceil(1000),
            Evidence::SIZE_LIMIT / 1000
        ),
    }
}

/// The evidence each request `ai` was asked carried, in order.
fn evidence_asked(ai: &ScriptedAi) -> Vec<Evidence> {
    ai.asked()
        .into_iter()
        .map(|asked| match asked {
            Asked::CheckAreas { evidence, .. }
            | Asked::WriteCandidates { evidence, .. }
            | Asked::CheckQuestion { evidence, .. }
            | Asked::AssessReasoning { evidence, .. }
            | Asked::Explain { evidence, .. }
            | Asked::CheckExplanation { evidence, .. } => evidence,
        })
        .collect()
}

/// What each request `ai` was asked sends, in order: the request the real
/// AI builds from what the scripted one was given, as its kind, the
/// evidence as sent, which the hint's check sends none of, and the whole
/// request as text.
fn requests_sent(ai: &ScriptedAi) -> Vec<(&'static str, Value, String)> {
    let model = ModelId::new("scripted".to_owned());
    ai.asked()
        .into_iter()
        .map(|asked| match asked {
            Asked::CheckAreas { sent, .. } => (
                "gate",
                sent.state["evidence"].clone(),
                format!("{} {:?}", sent.state, sent.questions),
            ),
            Asked::WriteCandidates {
                evidence,
                area,
                count,
            } => {
                let sent = candidates_request(&model, &evidence, &area, count.into());
                (
                    "write",
                    serde_json::from_str(&sent.input).expect("the input is JSON"),
                    format!("{} {}", sent.preamble, sent.input),
                )
            }
            Asked::CheckQuestion {
                call,
                evidence,
                candidate,
            } => {
                let sent = question_check_request(&model, &evidence, &candidate, call);
                (
                    match call {
                        QuestionCheckCall::OptionRatings => "check: option ratings",
                        QuestionCheckCall::Hint => "check: hint",
                        QuestionCheckCall::Candidate => "check: candidate",
                    },
                    sent.state["evidence"].clone(),
                    format!("{} {:?}", sent.state, sent.questions),
                )
            }
            Asked::AssessReasoning { .. }
            | Asked::Explain { .. }
            | Asked::CheckExplanation { .. } => {
                unreachable!("the tests that read what is sent submit no response")
            }
        })
        .collect()
}

/// Whether `evidence` sends nothing of `plan.txt`, as the learner left it
/// out.
fn leaves_out_the_plan(evidence: &Evidence) -> bool {
    evidence
        .files()
        .iter()
        .any(|file| file.path() == PLAN && file.sent().is_none())
}

/// The rows above Preparing questions' title, trimmed, without the blank
/// ones: the stage row.
fn stage_row(session: &Session) -> Vec<String> {
    let rows = session.rows();
    let title = rows
        .iter()
        .position(|row| row.trim() == "Preparing questions");
    assert!(
        title.is_some(),
        "no Preparing questions:\n{}",
        session.text()
    );
    rows.iter()
        .take(title.unwrap_or_default())
        .map(|row| row.trim().to_owned())
        .filter(|row| !row.is_empty())
        .collect()
}

/// Whether `session` is on the area and change screen, past the outbound
/// review.
fn on_the_picker(session: &Session) -> bool {
    let text = session.text();
    text.contains("Review what will be sent") && !text.contains("Sends to")
}

#[test]
fn send_opens_preparing_questions_and_asks_jev_which_areas_are_worth_asking_about() {
    let repo = doubling_the_charge();
    let ai = Arc::new(ScriptedAi::new());
    let mut session = reviewing(&repo, &ai, REVIEW_NEWEST, 120, 32);

    press_send(&mut session);

    let text = session.text();
    assert!(text.contains("Preparing questions"), "{text}");
    assert!(text.contains("Double the charge"), "{text}");
    let asked = ai.asked();
    assert!(
        matches!(asked.as_slice(), [Asked::CheckAreas { .. }]),
        "the relevance gate goes out first, before any reply: {asked:#?}"
    );
}

#[test]
fn preparing_questions_on_the_repository_ticks_the_outbound_review_in_its_stage_row() {
    let repo = doubling_the_charge();
    let ai = Arc::new(ScriptedAi::new());
    let mut session = reviewing(&repo, &ai, REVIEW_NEWEST, 120, 32);

    press_send(&mut session);

    assert_eq!(
        stage_row(&session),
        [
            "✓ Reading the change → ✓ Finding connected code → ✓ Preparing evidence → ✓ Outbound review →",
            "⠋ Checking areas · Jev → · Writing questions · generator → · Checking questions · Jev",
        ]
    );
}

#[test]
fn cancel_and_back_on_preparing_questions_return_to_the_area_and_change_screen() {
    let repo = doubling_the_charge();
    let ai = Arc::new(ScriptedAi::new());
    for back in ["<Enter>", "q", "<Esc>"] {
        let mut session = reviewing(&repo, &ai, REVIEW_NEWEST, 120, 32);
        press_send(&mut session);
        assert_eq!(session.row_with("Cancel"), "> Cancel");

        session.press(back);

        assert!(
            on_the_picker(&session),
            "{back} returns to the picker, past the outbound review:\n{}",
            session.text()
        );
    }
}

#[test]
fn every_request_carries_exactly_the_evidence_the_outbound_review_showed() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();
    let mut session = reviewing(&repo, &ai, REVIEW_ABOUT_THE_DOMAIN, 120, 60);
    session.press(LEAVE_OUT_THE_PLAN);
    let shown = shown(&mut session);
    assert_eq!(shown.files.len(), 3, "{shown:#?}");
    assert_eq!(
        shown.connections.len(),
        4,
        "two connected, a related test and the manifest: {shown:#?}"
    );
    assert_eq!(shown.areas.len(), 1, "{shown:#?}");

    press_send(&mut session);
    session.settle();
    assert!(
        session.text().contains("Question 1 of 1"),
        "{}",
        session.text()
    );
    submit(&mut session, "a", "The retry sends a new key.");
    session.settle();

    let asked = evidence_asked(&ai);
    assert_eq!(
        asked.len(),
        8,
        "the gate, one write, one check's three calls, the reasoning evaluation, the \
         explanation and its check"
    );
    for evidence in &asked {
        assert_eq!(as_shown(evidence), shown);
    }
    assert!(
        session
            .text()
            .contains("You didn't say the retry sends a new key."),
        "the explanation shows: {}",
        session.text()
    );
}

#[test]
fn a_file_the_learner_left_out_is_absent_from_every_request() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();

    sent_without_the_plan(&repo, &ai, 120, 32);

    let sent = requests_sent(&ai);
    assert_eq!(
        sent.iter().map(|(kind, ..)| *kind).collect::<Vec<_>>(),
        [
            "gate",
            "write",
            "check: option ratings",
            "check: hint",
            "check: candidate"
        ]
    );
    for (kind, evidence, text) in &sent {
        assert!(
            !text.contains(PLAN),
            "{kind} names the file left out:\n{text}"
        );
        assert!(!text.contains(PLAN_TEXT), "{kind} sends the file left out");
        if *kind == "check: hint" {
            assert_eq!(evidence, &Value::Null, "the hint's check sends no evidence");
            continue;
        }
        assert!(
            text.contains("notes.txt"),
            "{kind}: the other files are sent"
        );
        let touched: Vec<&str> = evidence["touched_files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|file| file["path"].as_str())
            .collect();
        assert_eq!(touched, ["notes.txt", "src/pay.rs"], "{kind}");
    }
}

#[test]
fn what_jev_and_the_generator_receive_names_the_files_without_connected_code() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();

    sent_without_the_plan(&repo, &ai, 120, 32);

    let sent = requests_sent(&ai);
    assert_eq!(
        sent.len(),
        5,
        "the gate, one write and one check's three calls"
    );
    for (kind, evidence, _) in sent.iter().filter(|(kind, ..)| *kind != "check: hint") {
        assert_eq!(
            evidence["files_without_connected_code"],
            json!(["notes.txt"]),
            "{kind}: src/pay.rs has connected code, and the file left out isn't named"
        );
    }
}

#[test]
fn retry_after_the_gate_fails_sends_the_same_evidence_with_no_new_outbound_review() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();
    ai.on_gate_failing(RequestFailure::RateLimited);
    let mut session = sent_without_the_plan(&repo, &ai, 120, 32);
    assert_eq!(session.row_with("> Retry"), "> Retry");

    session.press("<Enter>");
    session.settle();

    let asked = evidence_asked(&ai);
    assert_eq!(
        asked.len(),
        6,
        "the gate twice, one write and one check's three calls"
    );
    assert!(leaves_out_the_plan(&asked[0]), "{:#?}", asked[0]);
    assert!(asked.iter().all(|evidence| *evidence == asked[0]));
    assert!(
        session.text().contains("Question 1 of 1"),
        "{}",
        session.text()
    );
}

#[test]
fn ask_anyway_sends_the_same_evidence_with_no_new_outbound_review() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();
    ai.on_gate(
        &area_id(DOMAIN),
        Probability::new(0.1).expect("from 0 to 1"),
    );
    let mut session = sent_without_the_plan(&repo, &ai, 120, 32);
    assert!(
        session
            .text()
            .contains("Nothing to practice in this change"),
        "{}",
        session.text()
    );

    session.press("j j");
    assert_eq!(session.row_with("> Ask anyway"), "> Ask anyway");
    session.press("<Enter>");
    session.settle();

    let asked = evidence_asked(&ai);
    assert_eq!(
        asked.len(),
        5,
        "the gate, then one write and one check's three calls"
    );
    assert!(leaves_out_the_plan(&asked[0]), "{:#?}", asked[0]);
    assert!(asked.iter().all(|evidence| *evidence == asked[0]));
    assert!(
        session.text().contains("Question 1 of 1"),
        "{}",
        session.text()
    );
}

#[test]
fn back_from_practice_keeps_the_learners_choices_for_the_change() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();
    let mut session = sent_without_the_plan(&repo, &ai, 120, 32);
    assert!(
        session.text().contains("Question 1 of 1"),
        "{}",
        session.text()
    );

    session.press("q");
    assert!(on_the_picker(&session), "{}", session.text());
    session.press("<Enter>");

    assert_eq!(
        session.row_with(PLAN),
        format!("{PLAN}    1 KB  left out by you")
    );
}

#[test]
fn without_a_key_send_says_it_needs_one_before_any_other_reason_and_sends_nothing() {
    let repo = with_a_secret();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.settle();
    session.press(TO_THE_CHANGE);
    session.press("<S-Tab> k");
    assert_eq!(
        session.row_with("Possible secret"),
        "Possible secret in config.txt, line 1: AWS access key ID, AKIA********"
    );

    assert_eq!(send(&session), "Send (needs OPENROUTER_API_KEY)");
    session.press("<Enter> <Space>");
    assert_eq!(
        session.row_with("> Send"),
        "> Send (needs OPENROUTER_API_KEY)",
        "still on the outbound review"
    );
}

#[test]
fn with_a_key_send_keeps_its_reasons_from_the_evidence_and_sends_nothing_while_one_holds() {
    let repo = with_a_secret();
    let ai = Arc::new(ScriptedAi::new());
    let mut session = reviewing(&repo, &ai, REVIEW_NEWEST, 120, 32);
    session.press(TO_THE_CHANGE);
    session.press("<S-Tab> k");

    assert_eq!(send(&session), "Send (resolve the possible secrets)");
    session.press("<Enter> <Space>");
    assert_eq!(
        session.row_with("> Send"),
        "> Send (resolve the possible secrets)",
        "still on the outbound review"
    );
    assert!(ai.asked().is_empty(), "{:#?}", ai.asked());
}

#[test]
fn send_waits_for_connected_code_and_once_its_time_limit_runs_out_sends_without_it() {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();
    let mut session = Session::start(
        Setup {
            time_limit: Some(Duration::ZERO),
            ..setup(&repo, &ai)
        },
        120,
        32,
    );
    session.press(REVIEW_ABOUT_THE_DOMAIN);
    session.press("<S-Tab> k");
    assert_eq!(send(&session), "Send (finding connected code)");
    session.press("<Enter>");
    assert!(ai.asked().is_empty(), "{:#?}", ai.asked());

    session.wait(Duration::ZERO);
    assert_eq!(
        session.row_with("Connected code"),
        "Connected code isn't available for this change: finding it took longer than 0 s."
    );
    press_send(&mut session);
    assert_eq!(
        stage_row(&session),
        [
            "✓ Reading the change → – Finding connected code → ✓ Preparing evidence → ✓ Outbound review →",
            "⠋ Checking areas · Jev → · Writing questions · generator → · Checking questions · Jev",
        ]
    );
    session.settle();

    let sent = requests_sent(&ai);
    assert_eq!(
        sent.len(),
        5,
        "the gate, one write and one check's three calls"
    );
    for (kind, evidence, _) in sent.iter().filter(|(kind, ..)| *kind != "check: hint") {
        assert_eq!(evidence["connected_code"], json!([]), "{kind}");
        assert_eq!(
            evidence["files_without_connected_code"],
            json!(["notes.txt", "plan.txt", "src/pay.rs"]),
            "{kind}"
        );
    }
}

/// A session `width` by `height` on Preparing questions on the learner's
/// repository, sent from the outbound review, while Jev checks the areas.
fn checking_areas_on_the_repository(width: u16, height: u16) -> Session {
    let repo = doubling_the_charge();
    let ai = writing_one_about_the_domain();
    let mut session = reviewing(&repo, &ai, REVIEW_ABOUT_THE_DOMAIN, width, height);
    press_send(&mut session);
    session
}

#[test]
fn preparing_questions_on_the_repository_while_jev_checks_the_areas_at_120x32() {
    insta::assert_debug_snapshot!(
        "repository_checking_areas_120x32",
        checking_areas_on_the_repository(120, 32).frame()
    );
}

#[test]
fn preparing_questions_on_the_repository_while_jev_checks_the_areas_at_80x24() {
    insta::assert_debug_snapshot!(
        "repository_checking_areas_80x24",
        checking_areas_on_the_repository(80, 24).frame()
    );
}
