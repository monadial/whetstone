//! Provenance in the session record: every request Practice sends is saved
//! with what produced its result, as the result arrives. The record starts
//! with each request that ended before the questions showed (the relevance
//! gate, writing and each question check's three calls, the candidate call
//! holding the depth), and each later one is saved as it ends (the
//! reasoning's evaluation, the explanation and its check), before the
//! outcomes it gives.
//!
//! Each request's provenance holds Whetstone's own identity for it, counting
//! from 1 in the order sent; the models asked and answering; the versions
//! behind it: Whetstone's, the base prompts', the assessment policy's, and
//! each area file's, with its source and a hash of its content; for Jev,
//! every question sent exactly as sent with its answer as returned; for the
//! generator, `OpenRouter`'s generation ID; and how the request ended.

use std::fmt::Write as _;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use whetstone::{
    Asking, Assessment, ExplanationCheck, NewRecord, RequestEnded, SentRequest, StoreEntry,
};
use whetstone_ai::{
    Asked, JevQuestion, JevRequest, ParagraphCheck, QuestionCheckCall, QuestionWithoutKey,
    RequestFailure, ScriptedAi, WrittenParagraph, base_prompts_hash, explanation_check_request,
    question_check_request, reasoning_request,
};
use whetstone_core::{
    Area, AreaFileVersion, AreaSource, AssessmentPolicy, ContentHash, CriterionOutcome,
    GenerationId, JevExchange, ModelId, Probability, RequestId, Role, WrittenCandidate,
    WrittenCitation,
};

use crate::answering::{answering, submit};
use crate::connected_code::{rust_pay, rust_payments};
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, generate, stems, touched_path,
    writing_about_the_domain,
};
use crate::session::{NO_COLOR, Session, Setup};

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// The reasoning the tests submit.
const REASONING: &str = "Retries reuse the key, so the provider dedupes them.";

/// The model the scripted Jev says it is.
const SCRIPTED_JEV: &str = "scripted/jev";

fn probability(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// The version of Whetstone's default file for the domain area: the
/// SHA-256 hash of its bytes, as `shasum -a 256` prints it.
fn domain_file() -> AreaFileVersion {
    let (_, contents) = Area::default_files()
        .iter()
        .find(|(file_name, _)| *file_name == "domain.md")
        .expect("Whetstone has a default domain area file");
    let hex = Sha256::digest(contents)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("writing to a String never fails");
            hex
        });
    AreaFileVersion::new(
        area_id(DOMAIN),
        AreaSource::Default,
        ContentHash::new(hex).expect("lowercase hexadecimal"),
    )
}

/// Asserts that `request` went out behind this Whetstone, its base prompts,
/// the current assessment policy and the domain area's default file.
fn assert_versions(request: &SentRequest) {
    let versions = request.provenance.versions();
    assert_eq!(versions.whetstone(), env!("CARGO_PKG_VERSION"));
    assert_eq!(versions.base_prompts(), &base_prompts_hash());
    assert_eq!(versions.policy(), AssessmentPolicy::current());
    assert_eq!(
        versions.area_files(),
        [domain_file()],
        "{:?}",
        request.asking
    );
}

/// Asserts that `exchanges` keep each question in `sent` exactly as it was
/// sent, in name order.
fn assert_kept_as_sent(exchanges: &[JevExchange], sent: &JevRequest) {
    let names: Vec<&str> = exchanges.iter().map(JevExchange::name).collect();
    let asked: Vec<&str> = sent.questions.keys().map(String::as_str).collect();
    assert_eq!(names, asked);
    for exchange in exchanges {
        match (exchange, &sent.questions[exchange.name()]) {
            (
                JevExchange::YesNo {
                    instructions,
                    yes,
                    no,
                    ..
                },
                JevQuestion::YesNo {
                    instructions: sent,
                    meanings,
                },
            ) => {
                assert_eq!(instructions, sent);
                assert_eq!(
                    yes.as_ref(),
                    meanings.as_ref().map(|meanings| &meanings.yes)
                );
                assert_eq!(no.as_ref(), meanings.as_ref().map(|meanings| &meanings.no));
            }
            (
                JevExchange::Score {
                    instructions,
                    levels,
                    ..
                },
                JevQuestion::Score {
                    instructions: sent,
                    levels: sent_levels,
                },
            ) => {
                assert_eq!(instructions, sent);
                assert_eq!(levels, sent_levels);
            }
            (
                JevExchange::Choice {
                    instructions,
                    options,
                    ..
                },
                JevQuestion::Choice {
                    instructions: sent,
                    options: sent_options,
                },
            ) => {
                assert_eq!(instructions, sent);
                assert_eq!(options, sent_options);
            }
            (exchange, question) => {
                unreachable!("{exchange:?} is kept for {question:?}, a question of another kind")
            }
        }
    }
}

/// Each yes/no answer `request` keeps, by its question's name.
fn answers(request: &SentRequest) -> Vec<(&str, f64)> {
    request
        .provenance
        .exchanges()
        .iter()
        .filter_map(|exchange| match exchange {
            JevExchange::YesNo { name, answer, .. } => Some((name.as_str(), answer.get())),
            JevExchange::Score { .. } | JevExchange::Choice { .. } => None,
        })
        .collect()
}

/// The record the session started.
fn started(session: &Session) -> NewRecord {
    session
        .store()
        .log()
        .into_iter()
        .find_map(|entry| match entry {
            StoreEntry::Started { record, .. } => Some(record),
            _ => None,
        })
        .expect("the record started")
}

/// Each request saved after the record started, in order.
fn saved_requests(session: &Session) -> Vec<SentRequest> {
    session
        .store()
        .log()
        .into_iter()
        .filter_map(|entry| match entry {
            StoreEntry::Request { request, .. } => Some(request),
            _ => None,
        })
        .collect()
}

#[test]
fn the_record_starts_with_each_request_before_the_questions_with_its_provenance() {
    let ai = writing_about_the_domain(4, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);

    session.settle();

    let record = started(&session);
    let checks: Vec<Asking> = record
        .questions
        .iter()
        .flat_map(|question| {
            QuestionCheckCall::ALL.map(|call| Asking::Check(question.id().clone(), call))
        })
        .collect();
    let asking: Vec<&Asking> = record.requests.iter().map(|sent| &sent.asking).collect();
    assert_eq!(
        asking[..2],
        [&Asking::Gate, &Asking::Write(area_id(DOMAIN))]
    );
    assert_eq!(
        asking[2..].len(),
        12,
        "each call of each question's check: {asking:?}"
    );
    for check in &checks {
        assert!(asking.contains(&check), "{check:?}");
    }
    let mut identities: Vec<u64> = record
        .requests
        .iter()
        .map(|sent| sent.provenance.request().get())
        .collect();
    assert_eq!(identities[..2], [1, 2], "the gate, then the write");
    identities.sort_unstable();
    assert_eq!(
        identities,
        (1..=14).collect::<Vec<u64>>(),
        "each its own, in the order sent"
    );
    for sent in &record.requests {
        assert_eq!(sent.ended, RequestEnded::Replied, "{:?}", sent.asking);
        assert_versions(sent);
    }

    let [gate, write, ..] = record.requests.as_slice() else {
        panic!("the gate and the write: {:?}", record.requests);
    };
    let jev = ModelId::new(SCRIPTED_JEV.to_owned());
    assert_eq!(gate.provenance.role(), Role::Jev);
    assert_eq!(gate.provenance.requested_model(), &jev);
    assert_eq!(gate.provenance.reported_model(), Some(&jev));
    let asked = ai.asked();
    let Some(Asked::CheckAreas { sent, .. }) = asked.first() else {
        panic!("the gate first: {asked:#?}");
    };
    assert_kept_as_sent(gate.provenance.exchanges(), sent);
    assert_eq!(answers(gate), [(area_id(DOMAIN).as_str(), 0.9)]);
    assert_eq!(write.provenance.role(), Role::Generator);
    assert_eq!(
        write.provenance.exchanges(),
        [],
        "the generator answers none"
    );
    assert_eq!(
        write.provenance.generation(),
        Some(&GenerationId::new("scripted-generation-2".to_owned()))
    );
}

#[test]
fn each_call_of_each_check_is_in_the_record_with_every_question_it_sent_and_its_answer() {
    let ai = writing_about_the_domain(4, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);

    session.settle();

    let record = started(&session);
    let jev = ModelId::new(SCRIPTED_JEV.to_owned());
    for asked in &ai.asked() {
        let Asked::CheckQuestion {
            call,
            evidence,
            candidate,
        } = asked
        else {
            continue;
        };
        let check = record
            .requests
            .iter()
            .find(|sent| sent.asking == Asking::Check(candidate.id().clone(), *call))
            .expect("each call of each check is in the record");
        assert_eq!(check.provenance.role(), Role::Jev);
        assert_eq!(check.provenance.reported_model(), Some(&jev));
        let request = question_check_request(&jev, evidence, candidate, *call);
        assert_kept_as_sent(check.provenance.exchanges(), &request);
        let depth = check
            .provenance
            .exchanges()
            .iter()
            .find_map(|exchange| match exchange {
                JevExchange::Score { name, answer, .. } if name == "depth" => Some(*answer),
                JevExchange::YesNo { .. }
                | JevExchange::Score { .. }
                | JevExchange::Choice { .. } => None,
            });
        let question = record
            .questions
            .iter()
            .find(|question| question.id() == candidate.id())
            .expect("each checked candidate passed");
        match call {
            QuestionCheckCall::OptionRatings => {
                assert!(answers(check).contains(&("option_a", 0.9)), "{check:?}");
                assert!(answers(check).contains(&("option_b", 0.1)), "{check:?}");
                assert_eq!(depth, None);
            }
            QuestionCheckCall::Hint => {
                assert_eq!(answers(check), [("hint_hides_answer_key", 0.9)]);
                assert_eq!(depth, None);
            }
            QuestionCheckCall::Candidate => {
                assert!(answers(check).contains(&("answerable", 0.9)), "{check:?}");
                assert_eq!(
                    depth,
                    Some(question.depth()),
                    "the depth the question holds, with the candidate call"
                );
            }
        }
    }
}

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, each passing every check, the first's
/// reasoning assessed as `assessment`.
fn writing(assessment: Result<Vec<Probability>, RequestFailure>) -> Arc<ScriptedAi> {
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
    ai.on_assess(FIRST, assessment);
    ai
}

/// The kind of each entry in `log`, with what a request asked.
fn kinds(log: &[StoreEntry]) -> Vec<String> {
    log.iter()
        .map(|entry| match entry {
            StoreEntry::Started { .. } => "started".to_owned(),
            StoreEntry::Response { .. } => "response".to_owned(),
            StoreEntry::Assessment { assessment, .. } => match assessment {
                Assessment::Choice(_) => "choice".to_owned(),
                Assessment::Criterion { .. } => "criterion".to_owned(),
                Assessment::Explanation { .. } => "explanation".to_owned(),
            },
            StoreEntry::Signal { .. } => "signal".to_owned(),
            StoreEntry::Request { request, .. } => match request.asking {
                Asking::Gate => "request: gate",
                Asking::Write(_) => "request: write",
                Asking::Check(..) => "request: check",
                Asking::Evaluate(_) => "request: evaluate",
                Asking::Explain(_) => "request: explain",
                Asking::CheckExplanation(_) => "request: check explanation",
            }
            .to_owned(),
        })
        .collect()
}

/// [`writing`], with the first's reasoning missing its criterion, at 0.1,
/// and the generator's explanation of it passing Jev's check, at 0.93 and
/// 0.88.
fn explaining_the_first() -> Arc<ScriptedAi> {
    let ai = writing(Ok(vec![probability(0.1)]));
    ai.on_explain(
        FIRST,
        Ok(vec![WrittenParagraph {
            criterion: 0,
            outcome: CriterionOutcome::NotMet,
            text: "You didn't say why reusing the key stops a second charge.".to_owned(),
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
            supported: probability(0.93),
            agrees: probability(0.88),
        }]),
    );
    ai
}

#[test]
fn each_later_request_is_saved_as_it_ends_before_the_outcomes_it_gives() {
    let ai = explaining_the_first();
    let mut session = answering(&ai, 120, 32);

    submit(&mut session, "a", REASONING);
    session.settle();

    let log = session.store().log();
    assert_eq!(
        kinds(&log),
        [
            "started",
            "response",
            "choice",
            "request: evaluate",
            "criterion",
            "request: explain",
            "request: check explanation",
            "explanation",
        ]
    );
    let Some(StoreEntry::Response { saved, .. }) = log.get(1) else {
        panic!("the response: {log:#?}");
    };
    let [evaluate, explain, check] = saved_requests(&session).try_into().expect("three requests");
    assert_eq!(evaluate.asking, Asking::Evaluate(saved.clone()));
    assert_eq!(explain.asking, Asking::Explain(saved.clone()));
    assert_eq!(check.asking, Asking::CheckExplanation(saved.clone()));
    assert_eq!(
        [&evaluate, &explain, &check].map(|sent| sent.provenance.request()),
        [15, 16, 17].map(RequestId::new),
        "after the gate, the write and four checks of three calls each"
    );
    for sent in [&evaluate, &explain, &check] {
        assert_eq!(sent.ended, RequestEnded::Replied);
        assert_versions(sent);
    }
    assert_eq!(answers(&evaluate), [("criterion_1", 0.1)]);
    let jev = ModelId::new(SCRIPTED_JEV.to_owned());
    let record = started(&session);
    let question = record
        .questions
        .iter()
        .find(|question| question.stem() == FIRST)
        .expect("the first question is in the record");
    assert_kept_as_sent(
        evaluate.provenance.exchanges(),
        &reasoning_request(
            &jev,
            &record.evidence,
            QuestionWithoutKey::from(question),
            REASONING,
        ),
    );
    assert_eq!(explain.provenance.role(), Role::Generator);
    assert_eq!(
        explain.provenance.generation(),
        Some(&GenerationId::new("scripted-generation-16".to_owned()))
    );
    assert_eq!(
        answers(&check),
        [
            ("criterion_1_agrees", 0.88),
            ("criterion_1_supported", 0.93)
        ]
    );
    let asked = ai.asked();
    let Some(Asked::CheckExplanation { paragraphs, .. }) = asked.last() else {
        panic!("the explanation's check last: {asked:#?}");
    };
    assert_kept_as_sent(
        check.provenance.exchanges(),
        &explanation_check_request(&jev, &record.evidence, question, paragraphs),
    );
    let Some(StoreEntry::Assessment {
        assessment: Assessment::Explanation {
            check: explained, ..
        },
        ..
    }) = log.last()
    else {
        panic!("the explanation last: {log:#?}");
    };
    assert!(
        matches!(explained, ExplanationCheck::Passed { .. }),
        "{explained:?}"
    );
}

#[test]
fn a_request_that_fails_is_saved_with_its_failure_and_no_reply() {
    let ai = writing(Err(RequestFailure::TimedOut));
    let mut session = answering(&ai, 120, 32);

    submit(&mut session, "a", REASONING);
    session.settle();

    let [evaluate] = saved_requests(&session).try_into().expect("one request");
    assert_eq!(
        evaluate.ended,
        RequestEnded::Failed(RequestFailure::TimedOut)
    );
    assert_eq!(evaluate.provenance.request(), RequestId::new(15));
    assert_eq!(evaluate.provenance.role(), Role::Jev);
    assert_eq!(
        evaluate.provenance.requested_model().as_str(),
        "typesafe/jev-1.14",
        "the model the config asks for"
    );
    assert_eq!(evaluate.provenance.reported_model(), None);
    assert_eq!(evaluate.provenance.exchanges(), []);
    assert_eq!(evaluate.provenance.cost(), None);
    assert_versions(&evaluate);
}

#[test]
fn a_request_still_running_when_the_learner_leaves_is_saved_as_cancelled() {
    let ai = writing(Ok(vec![probability(0.9)]));
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", REASONING);

    // Back to the areas and changes, before Jev's answers come.
    session.press("q");
    session.settle();

    let [evaluate] = saved_requests(&session).try_into().expect("one request");
    assert!(
        matches!(evaluate.asking, Asking::Evaluate(_)),
        "{evaluate:?}"
    );
    assert_eq!(
        evaluate.ended,
        RequestEnded::Failed(RequestFailure::Cancelled)
    );
    assert_eq!(evaluate.provenance.request(), RequestId::new(15));
}

#[test]
fn every_request_in_a_run_has_its_own_identity_across_practices() {
    let ai = writing_about_the_domain(4, PAYMENT_LIFECYCLE);
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
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    session.settle();

    // Back to the areas and changes, then Generate questions again.
    session.press("q");
    activate(&mut session, "Generate questions");
    session.settle();

    let records: Vec<Vec<u64>> = session
        .store()
        .log()
        .into_iter()
        .filter_map(|entry| match entry {
            StoreEntry::Started { record, .. } => Some(
                record
                    .requests
                    .iter()
                    .map(|sent| sent.provenance.request().get())
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    let [first, second] = records.as_slice() else {
        panic!("two records: {records:?}");
    };
    assert_eq!(first[..2], [1, 2]);
    assert_eq!(second[..2], [15, 16], "after the first Practice's fourteen");
}

#[test]
fn an_evaluation_the_learner_cancels_is_saved_as_cancelled() {
    let ai = writing(Ok(vec![probability(0.9)]));
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", REASONING);

    // Cancel evaluation sits above Next question, where focus starts.
    session.press("k <Enter>");
    session.press("j");
    session.settle();

    let [evaluate] = saved_requests(&session).try_into().expect("one request");
    assert_eq!(
        evaluate.asking,
        Asking::Evaluate(match &session.store().log()[1] {
            StoreEntry::Response { saved, .. } => saved.clone(),
            entry => panic!("the response: {entry:?}"),
        })
    );
    assert_eq!(
        evaluate.ended,
        RequestEnded::Failed(RequestFailure::Cancelled)
    );
    assert_eq!(evaluate.provenance.request(), RequestId::new(15));
    assert_eq!(evaluate.provenance.reported_model(), None);
}

#[test]
fn an_area_file_from_the_repository_goes_into_each_request_with_its_source_and_hash() {
    let domain = "+++\n\
                  name = \"Domain\"\n\
                  description = \"The rules the shop keeps\"\n\
                  +++\n\
                  Focus on what a charge must never do.\n";
    let repo = rust_payments();
    repo.write(".whetstone/areas/domain.md", domain);
    repo.commit("Define the domain area");
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    repo.commit("Double the charge");
    let ai = Arc::new(ScriptedAi::new());
    let stem = "Why does the charge double?";
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![WrittenCandidate {
            citations: vec![WrittenCitation {
                path: "src/pay.rs".to_owned(),
                first_line: 1,
                last_line: 1,
            }],
            ..candidate(stem, "A", PAYMENT_LIFECYCLE)
        }]),
    );
    ai.on_check_passing(stem);
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            start: Some(repo.root()),
            ai: Some(&ai),
            ..Setup::default()
        },
        120,
        32,
    );
    // The repository's one area is chosen; review its newest commit.
    session.press("<Enter> j <Space> k <S-Tab> <Enter>");
    session.settle();
    // Up to the change, round to Back, then up to Send.
    session.press(&"k ".repeat(20));
    session.press("<S-Tab> k");
    assert_eq!(session.row_with("> Send"), "> Send", "Send is enabled");
    session.press("<Enter>");
    session.settle();

    let record = started(&session);
    let digest = Sha256::digest(domain.as_bytes());
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").expect("writing to a String never fails");
        hex
    });
    let file = AreaFileVersion::new(
        area_id(DOMAIN),
        AreaSource::Repository,
        ContentHash::new(hex).expect("lowercase hexadecimal"),
    );
    assert_eq!(
        record.requests.len(),
        5,
        "the gate, the write and the check's three calls"
    );
    for sent in &record.requests {
        assert_eq!(
            sent.provenance.versions().area_files(),
            std::slice::from_ref(&file),
            "{:?}",
            sent.asking
        );
    }
}
