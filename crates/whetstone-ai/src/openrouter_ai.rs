//! The AI port's real implementation: Practice's requests to Jev and the
//! generator, sent through `OpenRouter` on Rig.
//!
//! Each call builds its request from the evidence before it returns, so the
//! request it hands back owns everything it sends ([`Pending`]). The reply
//! is decoded strictly (rule 37), and its provenance is the caller's stamp,
//! the model asked for, the model that answered, for Jev each question sent
//! with its answer as returned, for the generator `OpenRouter`'s generation
//! ID, and the usage and cost the reply reports. A failure is one of the
//! port's [`RequestFailure`]s.

use std::num::NonZeroUsize;

use tokio_util::sync::CancellationToken;
use whetstone_core::{
    Area, AreaId, Candidate, Cost, CriterionOutcome, Evidence, GenerationId, ModelId, Probability,
    Provenance, Question, Role, WrittenCandidate, shuffle_options,
};

use crate::candidates::{CandidatesOutput, candidates_request};
use crate::connection::OpenRouter;
use crate::exchanges::jev_exchanges;
use crate::explanation::{
    ExplanationOutput, ParagraphCheck, WrittenParagraph, explanation_check_answers,
    explanation_check_request, explanation_paragraphs, explanation_request,
};
use crate::failure::AiError;
use crate::gate::{gate_answers, gate_request};
use crate::jev::{JevReply, JevRequest};
use crate::port::{Ai, CandidateCount, Pending, Reply, RequestFailure, Saved, Stamp};
use crate::question_check::{
    CheckCallAnswers, QuestionCheckCall, question_check_answers, question_check_request,
};
use crate::reasoning::{QuestionWithoutKey, reasoning_answers, reasoning_request};
use crate::usage::Usage;

/// The models Practice asks: Jev's, as `[jev] model` names it, and the
/// generator's, as `[generator] model` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    /// The model Jev runs on, such as `typesafe/jev-1.14`.
    pub jev: ModelId,
    /// The generator's model, such as `anthropic/claude-sonnet-5.5`.
    pub generator: ModelId,
}

/// Jev and the generator on `OpenRouter`: the [`Ai`] Practice asks when a key
/// is found. Its `Debug` output redacts the key (rule 52).
#[derive(Debug, Clone)]
pub struct OpenRouterAi {
    connection: OpenRouter,
    models: Models,
}

impl OpenRouterAi {
    /// Practice's requests through `connection`, to `models`.
    #[must_use]
    pub fn new(connection: OpenRouter, models: Models) -> Self {
        Self { connection, models }
    }
}

/// A request's token never fires: a job cancels its request by dropping it
/// where it waits, which drops the HTTP request with it
/// (`Jobs::start_async` in the binary). Its time limit still applies.
fn never_cancelled() -> CancellationToken {
    CancellationToken::new()
}

impl Ai for OpenRouterAi {
    fn check_areas(
        &self,
        stamp: Stamp,
        evidence: &Evidence,
        areas: &[Area],
    ) -> Pending<Vec<(AreaId, Probability)>> {
        let request = gate_request(&self.models.jev, evidence, areas);
        let areas = areas.to_vec();
        let connection = self.connection.clone();
        let model = self.models.jev.clone();
        Box::pin(async move {
            let reply = connection.evaluate(&request, &never_cancelled()).await?;
            let content = gate_answers(&areas, &reply)?;
            let provenance = jev_provenance(stamp, model, &request, reply)?;
            log_reply(&provenance);
            Ok(Reply {
                content,
                provenance: Box::new(provenance),
            })
        })
    }

    fn write_candidates(
        &self,
        stamp: Stamp,
        evidence: &Evidence,
        area: &Area,
        count: CandidateCount,
    ) -> Pending<Vec<WrittenCandidate>> {
        let request = candidates_request(
            &self.models.generator,
            evidence,
            area,
            NonZeroUsize::from(count),
        );
        let connection = self.connection.clone();
        let model = self.models.generator.clone();
        Box::pin(async move {
            let reply = connection
                .generate::<CandidatesOutput>(&request, &never_cancelled())
                .await?;
            let provenance = reported(
                generated(stamp, model, reply.generation_id),
                reply.model,
                reply.usage,
            );
            log_reply(&provenance);
            Ok(Reply {
                content: reply
                    .output
                    .into_written()
                    .into_iter()
                    .map(shuffle_options)
                    .collect(),
                provenance: Box::new(provenance),
            })
        })
    }

    fn check_question(
        &self,
        stamp: Stamp,
        call: QuestionCheckCall,
        evidence: &Evidence,
        candidate: &Candidate,
    ) -> Pending<CheckCallAnswers> {
        let request = question_check_request(&self.models.jev, evidence, candidate, call);
        let candidate = candidate.clone();
        let connection = self.connection.clone();
        let model = self.models.jev.clone();
        Box::pin(async move {
            let reply = connection.evaluate(&request, &never_cancelled()).await?;
            let content = question_check_answers(&candidate, call, &reply)?;
            let provenance = jev_provenance(stamp, model, &request, reply)?;
            log_reply(&provenance);
            Ok(Reply {
                content,
                provenance: Box::new(provenance),
            })
        })
    }

    fn assess_reasoning(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: QuestionWithoutKey<'_>,
    ) -> Pending<Vec<Probability>> {
        let request = reasoning_request(&self.models.jev, evidence, question, saved.reasoning());
        let rubric = question.rubric().to_vec();
        let connection = self.connection.clone();
        let model = self.models.jev.clone();
        Box::pin(async move {
            let reply = connection.evaluate(&request, &never_cancelled()).await?;
            let content = reasoning_answers(&rubric, &reply)?;
            let provenance = jev_provenance(stamp, model, &request, reply)?;
            log_reply(&provenance);
            Ok(Reply {
                content,
                provenance: Box::new(provenance),
            })
        })
    }

    fn explain(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: &Question,
        criteria: &[(usize, CriterionOutcome)],
    ) -> Pending<Vec<WrittenParagraph>> {
        // The request explains each criterion of the rubric that isn't
        // met, so a criterion not asked about counts as met here: nothing
        // about it is sent. One outside the rubric makes the request
        // invalid.
        let mut outcomes = vec![CriterionOutcome::Met; question.rubric().len()];
        let in_rubric = criteria.iter().all(|&(criterion, outcome)| {
            outcomes
                .get_mut(criterion)
                .map(|slot| *slot = outcome)
                .is_some()
        });
        let request = in_rubric
            .then(|| {
                explanation_request(
                    &self.models.generator,
                    evidence,
                    question,
                    saved.reasoning(),
                    &outcomes,
                )
            })
            .flatten();
        let connection = self.connection.clone();
        let model = self.models.generator.clone();
        Box::pin(async move {
            let request = request.ok_or(RequestFailure::InvalidRequest)?;
            let reply = connection
                .generate::<ExplanationOutput>(&request, &never_cancelled())
                .await?;
            let content = explanation_paragraphs(&outcomes, reply.output)?;
            let provenance = reported(
                generated(stamp, model, reply.generation_id),
                reply.model,
                reply.usage,
            );
            log_reply(&provenance);
            Ok(Reply {
                content,
                provenance: Box::new(provenance),
            })
        })
    }

    fn check_explanation(
        &self,
        stamp: Stamp,
        _saved: &dyn Saved,
        evidence: &Evidence,
        question: &Question,
        paragraphs: &[WrittenParagraph],
    ) -> Pending<Vec<ParagraphCheck>> {
        let request = explanation_check_request(&self.models.jev, evidence, question, paragraphs);
        let paragraphs = paragraphs.to_vec();
        let connection = self.connection.clone();
        let model = self.models.jev.clone();
        Box::pin(async move {
            let reply = connection.evaluate(&request, &never_cancelled()).await?;
            let content = explanation_check_answers(&paragraphs, &reply)?;
            let provenance = jev_provenance(stamp, model, &request, reply)?;
            log_reply(&provenance);
            Ok(Reply {
                content,
                provenance: Box::new(provenance),
            })
        })
    }
}

/// The provenance of Jev's `reply` to `request`, which was stamped `stamp`
/// and asked `model`: each question sent, exactly as sent, with its answer
/// as returned, and what the reply reports.
fn jev_provenance(
    stamp: Stamp,
    model: ModelId,
    request: &JevRequest,
    reply: JevReply,
) -> Result<Provenance, AiError> {
    let exchanges = jev_exchanges(request, &reply)?;
    Ok(reported(
        Provenance::new(stamp.request, Role::Jev, model, stamp.versions).with_exchanges(exchanges),
        Some(reply.model),
        reply.usage,
    ))
}

/// The provenance of a request to the generator stamped `stamp`, which
/// asked `model`, with `OpenRouter`'s `generation` ID, if the reply gave
/// one.
fn generated(stamp: Stamp, model: ModelId, generation: Option<String>) -> Provenance {
    let requested = Provenance::new(stamp.request, Role::Generator, model, stamp.versions);
    match generation {
        Some(id) => requested.with_generation(GenerationId::new(id)),
        None => requested,
    }
}

/// `requested` with what the reply reports: the model that answered, and its
/// usage and cost. A cost that isn't one (negative, say) counts as not
/// reported, since a cost is never estimated.
fn reported(requested: Provenance, reported_model: Option<String>, usage: Usage) -> Provenance {
    let provenance = requested.with_usage(whetstone_core::Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
    });
    let provenance = match reported_model {
        Some(model) => provenance.with_reported_model(ModelId::new(model)),
        None => provenance,
    };
    match usage.cost.and_then(|usd| Cost::new(usd).ok()) {
        Some(cost) => provenance.with_cost(cost),
        None => provenance,
    }
}

/// Logs a reply's `provenance`: its identities, models, token counts and
/// cost, and nothing that was sent or written, nor Jev's answers (rules 43
/// and 44).
fn log_reply(provenance: &Provenance) {
    tracing::info!(
        request = %provenance.request(),
        generation = provenance.generation().map(GenerationId::as_str),
        role = ?provenance.role(),
        model = %provenance.requested_model(),
        answered = provenance.reported_model().map(ModelId::as_str),
        input_tokens = provenance.usage().input_tokens,
        output_tokens = provenance.usage().output_tokens,
        cost_usd = provenance.cost().map(Cost::usd),
        "OpenRouter replied"
    );
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::Value;
    use whetstone_core::{
        CandidateCheck, CriterionOutcome, Depth, GenerationId, JevExchange, OptionLetter,
        Probability, QuestionCount, Usage,
    };

    use super::*;
    use crate::replay::{connection, received, server};
    use crate::samples::{
        Acknowledged, REASONING, area, area_file, candidate, evidence, generator_model, jev_model,
        observability, paragraphs, question, recorded_check, stamp, versions, with_output,
    };
    use crate::{AiError, ParagraphCheck, QuestionWithoutKey, Timeouts};

    const CANDIDATES: &str = include_str!("../fixtures/candidates.json");
    const GATE: &str = include_str!("../fixtures/gate.json");
    const REASONING_REPLY: &str = include_str!("../fixtures/reasoning.json");
    const EXPLANATION: &str = include_str!("../fixtures/explanation.json");
    const EXPLANATION_CHECK: &str = include_str!("../fixtures/explanation-check.json");

    fn models() -> Models {
        Models {
            jev: jev_model(),
            generator: generator_model(),
        }
    }

    /// The real AI on a local server that answers `route` with `status` and
    /// `reply`.
    async fn ai(route: &str, status: u16, reply: &str) -> (wiremock::MockServer, OpenRouterAi) {
        let server = server(route, status, reply, Duration::ZERO).await;
        let ai = OpenRouterAi::new(connection(&server, Timeouts::default()), models());
        (server, ai)
    }

    /// Two candidates for each area: 4 questions over 3 areas.
    fn two_per_area() -> CandidateCount {
        CandidateCount::per_area(QuestionCount::try_from(4).expect("from 3 to 5"), 3)
    }

    /// Asserts that `exchanges`, kept in a Jev reply's provenance, hold each
    /// question the request `body` sent Jev exactly as it was sent, in name
    /// order: its instructions and its meanings of yes and no, or its
    /// levels.
    fn assert_kept_as_sent(exchanges: &[JevExchange], body: &Value) {
        let sent = body["questions"]
            .as_object()
            .expect("the questions are an object");
        let names: Vec<&str> = exchanges.iter().map(JevExchange::name).collect();
        assert_eq!(names, sent.keys().map(String::as_str).collect::<Vec<_>>());
        for exchange in exchanges {
            let question = &sent[exchange.name()];
            match exchange {
                JevExchange::YesNo {
                    instructions,
                    yes,
                    no,
                    ..
                } => {
                    assert_eq!(question["instructions"], instructions.as_str());
                    assert_eq!(question["criteria"]["true"].as_str(), yes.as_deref());
                    assert_eq!(question["criteria"]["false"].as_str(), no.as_deref());
                }
                JevExchange::Score {
                    instructions,
                    levels,
                    ..
                } => {
                    assert_eq!(question["instructions"], instructions.as_str());
                    assert_eq!(question["criteria"], serde_json::json!(levels));
                }
            }
        }
    }

    /// Each yes/no answer in `exchanges`, by its question's name.
    fn yes_no_answers(exchanges: &[JevExchange]) -> Vec<(&str, f64)> {
        exchanges
            .iter()
            .filter_map(|exchange| match exchange {
                JevExchange::YesNo { name, answer, .. } => Some((name.as_str(), answer.get())),
                JevExchange::Score { .. } => None,
            })
            .collect()
    }

    /// `provenance` without the questions it keeps.
    fn without_exchanges(provenance: &Provenance) -> Provenance {
        provenance.clone().with_exchanges(Vec::new())
    }

    #[tokio::test]
    async fn the_generator_writes_candidates_with_the_models_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/chat/completions", 200, CANDIDATES).await;

        let reply = ai
            .write_candidates(stamp(), &evidence(), &area(), two_per_area())
            .await
            .expect("the recorded reply decodes");

        assert_eq!(reply.content.len(), 2);
        assert_eq!(
            reply.content[0].stem,
            "A charge request times out, but the provider had already recorded it before the \
             response was lost. What does charge_with_retry return after its second request?"
        );
        assert_eq!(
            *reply.provenance,
            Provenance::new(
                stamp().request,
                Role::Generator,
                generator_model(),
                versions()
            )
            .with_generation(GenerationId::new(
                "gen-1792574742-6mTYp6eLYi65k1QVjIP4".to_owned()
            ))
            .with_reported_model(ModelId::new("anthropic/claude-sonnet-5.5".to_owned()))
            .with_usage(Usage {
                input_tokens: Some(4818),
                output_tokens: Some(2713),
            })
            .with_cost(Cost::new(0.036_766).expect("0.036766 is a cost"))
        );
        let (_, body) = received(&server).await;
        assert_eq!(body["model"], generator_model().as_str());
        assert_eq!(
            body["provider"],
            serde_json::json!({ "zdr": true, "data_collection": "deny" })
        );
        assert_eq!(body.get("tools"), None::<&Value>);
    }

    #[tokio::test]
    async fn the_candidates_come_with_their_options_shuffled_each_rationale_and_the_answer_key_with_its_option()
     {
        let (_server, ai) = ai("/chat/completions", 200, CANDIDATES).await;

        let reply = ai
            .write_candidates(stamp(), &evidence(), &area(), two_per_area())
            .await
            .expect("the recorded reply decodes");

        // The recorded reply's second candidate has its correct option at A;
        // the shuffle moves it to C, and the option written at C to A.
        let second = &reply.content[1];
        assert_eq!(second.answer_key, "C");
        let options: Vec<(&str, &str, &str)> = second
            .options
            .iter()
            .map(|option| {
                (
                    option.letter.as_str(),
                    option.text.as_str(),
                    option.rationale.split('.').next().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            options,
            [
                (
                    "A",
                    "It confirms that the retry returns first receipt",
                    "Wrong"
                ),
                ("B", "It confirms that both requests carry one key", "Wrong"),
                (
                    "C",
                    "It confirms that two charge requests are made",
                    "Correct"
                ),
            ]
        );
    }

    /// The recorded reply to each call of the sample candidate's check, and
    /// the tokens and cost it reports.
    fn recorded_checks() -> [(QuestionCheckCall, &'static str, Usage, f64); 3] {
        let usage = |input, output| Usage {
            input_tokens: Some(input),
            output_tokens: Some(output),
        };
        let recorded = |call| recorded_check(call).1;
        [
            (
                QuestionCheckCall::OptionRatings,
                recorded(QuestionCheckCall::OptionRatings),
                usage(1563, 74),
                0.000_065_646,
            ),
            (
                QuestionCheckCall::Hint,
                recorded(QuestionCheckCall::Hint),
                usage(512, 24),
                0.000_021_504,
            ),
            (
                QuestionCheckCall::Candidate,
                recorded(QuestionCheckCall::Candidate),
                usage(2952, 250),
                0.000_123_984,
            ),
        ]
    }

    #[tokio::test]
    async fn jev_checks_a_candidate_in_three_calls_each_with_the_model_it_was_asked_and_what_it_cost()
     {
        let candidate = candidate();
        let mut answers = Vec::new();
        for (call, recorded, usage, cost) in recorded_checks() {
            let (server, ai) = ai("/systemone", 200, recorded).await;

            let reply = ai
                .check_question(stamp(), call, &evidence(), &candidate)
                .await
                .expect("the recorded reply decodes");

            let answered: Vec<CandidateCheck> =
                reply.content.iter().map(|(check, _)| check).collect();
            assert_eq!(answered, call.checks(&candidate), "{call:?}");
            assert_eq!(
                without_exchanges(&reply.provenance),
                Provenance::new(stamp().request, Role::Jev, jev_model(), versions())
                    .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                    .with_usage(usage)
                    .with_cost(Cost::new(cost).expect("a recorded cost is never negative")),
                "{call:?}"
            );
            let (_, body) = received(&server).await;
            assert_eq!(body["model"], jev_model().as_str());
            assert_eq!(body.get("provider"), None::<&Value>);
            answers.push(reply.content);
        }

        let answers = CheckCallAnswers::merged(answers).expect("each call's answers once");
        let answered: Vec<CandidateCheck> = answers.iter().map(|(check, _)| check).collect();
        assert_eq!(answered, candidate.checks(), "an answer to every check");
        let rating = |letter| {
            answers
                .get(CandidateCheck::Option(letter))
                .map(Probability::get)
        };
        assert_eq!(
            [
                rating(OptionLetter::A),
                rating(OptionLetter::B),
                rating(OptionLetter::C)
            ],
            [Some(0.99), Some(0.01), Some(0.02)]
        );
        assert_eq!(
            answers
                .get(CandidateCheck::NoGiveaway)
                .map(Probability::get),
            Some(0.2)
        );
        let probability = |value| Probability::new(value).expect("from 0 to 1");
        assert_eq!(
            answers.depth(),
            Depth::new([0.12, 0.3, 0.58].map(probability), 1.46).expect("from 0 to 2"),
            "the depth's distribution and expected value, as returned"
        );
    }

    #[tokio::test]
    async fn each_call_of_jev_s_question_check_keeps_each_question_sent_with_its_answer_as_returned()
     {
        let mut kept = Vec::new();
        for (call, recorded, ..) in recorded_checks() {
            let (server, ai) = ai("/systemone", 200, recorded).await;

            let reply = ai
                .check_question(stamp(), call, &evidence(), &candidate())
                .await
                .expect("the recorded reply decodes");

            let (_, body) = received(&server).await;
            let exchanges = reply.provenance.exchanges();
            assert_kept_as_sent(exchanges, &body);
            assert_eq!(
                exchanges.len(),
                call.checks(&candidate()).len() + usize::from(call.scores_depth()),
                "every question the {call:?} call sent, and only those"
            );
            kept.extend(exchanges.iter().cloned());
        }

        let answers = yes_no_answers(&kept);
        for answer in [
            ("option_a", 0.99),
            ("option_b", 0.01),
            ("no_giveaway", 0.2),
            ("hint_hides_answer_key", 0.27),
            ("criterion_2_minimum", 0.92),
        ] {
            assert!(
                answers.contains(&answer),
                "{answer:?}, as returned: {answers:?}"
            );
        }
        assert_eq!(answers.len(), 17, "every yes/no question: {answers:?}");
        let probability = |value| Probability::new(value).expect("from 0 to 1");
        let depth = kept.iter().find_map(|exchange| match exchange {
            JevExchange::Score { name, answer, .. } if name == "depth" => Some(*answer),
            JevExchange::YesNo { .. } | JevExchange::Score { .. } => None,
        });
        assert_eq!(
            depth,
            Some(Depth::new([0.12, 0.3, 0.58].map(probability), 1.46).expect("from 0 to 2")),
            "the depth's distribution and expected value, as returned"
        );
    }

    #[tokio::test]
    async fn jev_checks_the_chosen_areas_with_the_model_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/systemone", 200, GATE).await;
        let areas = [area(), observability()];

        let reply = ai
            .check_areas(stamp(), &evidence(), &areas)
            .await
            .expect("the recorded reply decodes");

        let answers: Vec<(&str, f64)> = reply
            .content
            .iter()
            .map(|(area, answer)| (area.as_str(), answer.get()))
            .collect();
        assert_eq!(
            answers,
            [("domain", 0.89), ("observability", 0.07)],
            "an answer per area, in the areas' order, as returned"
        );
        assert_eq!(
            without_exchanges(&reply.provenance),
            Provenance::new(stamp().request, Role::Jev, jev_model(), versions())
                .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(1234),
                    output_tokens: Some(39),
                })
                .with_cost(Cost::new(0.000_051_828).expect("0.000051828 is a cost"))
        );
        let (_, body) = received(&server).await;
        assert_kept_as_sent(reply.provenance.exchanges(), &body);
        assert_eq!(
            yes_no_answers(reply.provenance.exchanges()),
            [("domain", 0.89), ("observability", 0.07)],
            "each answer as returned"
        );
        assert_eq!(body["model"], jev_model().as_str());
        assert_eq!(body.get("provider"), None::<&Value>);
        let state: Vec<&str> = body["state"]
            .as_object()
            .expect("the state is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(state, ["evidence"], "the evidence alone");
        let questions: Vec<&str> = body["questions"]
            .as_object()
            .expect("the questions are an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            questions,
            ["domain", "observability"],
            "one question per area, named by its ID"
        );
        let sent = body.to_string();
        for area in areas {
            assert!(
                !sent.contains(area.prompt().trim()),
                "{}'s area prompt reached Jev",
                area.id()
            );
        }
    }

    #[tokio::test]
    async fn a_gate_reply_that_leaves_out_an_area_is_an_invalid_reply() {
        let (_server, ai) = ai("/systemone", 200, GATE).await;
        let security = area_file(
            "security.md",
            "+++\nname = \"Security\"\ndescription = \"Who can do what\"\n+++\n\nLook for checks.\n",
        );

        assert_eq!(
            ai.check_areas(stamp(), &evidence(), &[area(), observability(), security])
                .await
                .err(),
            Some(RequestFailure::InvalidReply)
        );
    }

    #[tokio::test]
    async fn jev_assesses_the_reasoning_per_criterion_with_the_model_it_was_asked_and_what_it_cost()
    {
        let (server, ai) = ai("/systemone", 200, REASONING_REPLY).await;
        let question = question();

        let reply = ai
            .assess_reasoning(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                QuestionWithoutKey::from(&question),
            )
            .await
            .expect("the recorded reply decodes");

        assert_eq!(
            reply.content,
            [0.75, 0.96].map(|answer| Probability::new(answer).expect("a probability"))
        );
        assert_eq!(
            without_exchanges(&reply.provenance),
            Provenance::new(stamp().request, Role::Jev, jev_model(), versions())
                .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(1475),
                    output_tokens: Some(44),
                })
                .with_cost(Cost::new(0.000_061_95).expect("0.00006195 is a cost"))
        );
        let (_, body) = received(&server).await;
        assert_kept_as_sent(reply.provenance.exchanges(), &body);
        assert_eq!(
            yes_no_answers(reply.provenance.exchanges()),
            [("criterion_1", 0.75), ("criterion_2", 0.96)]
        );
        assert_eq!(body["model"], jev_model().as_str());
        assert_eq!(body.get("provider"), None::<&Value>);
        let keys = |value: &Value| -> Vec<String> {
            value
                .as_object()
                .expect("an object")
                .keys()
                .cloned()
                .collect()
        };
        assert_eq!(keys(&body["state"]), ["evidence", "question", "reasoning"]);
        assert_eq!(keys(&body["state"]["question"]), ["stem", "options"]);
        assert_eq!(body["state"]["reasoning"], REASONING);
        assert_eq!(keys(&body["questions"]), ["criterion_1", "criterion_2"]);
        let sent = body.to_string();
        for letter in OptionLetter::ALL {
            assert!(
                !sent.contains(question.rationale(letter)),
                "the rationale for {letter}, which names the answer key, isn't sent"
            );
        }
        assert!(!sent.contains("answer_key"));
    }

    #[tokio::test]
    async fn a_reasoning_reply_that_answers_other_questions_is_an_invalid_reply() {
        let (_server, ai) = ai(
            "/systemone",
            200,
            recorded_check(QuestionCheckCall::Candidate).1,
        )
        .await;
        let question = question();

        assert_eq!(
            ai.assess_reasoning(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                QuestionWithoutKey::from(&question),
            )
            .await
            .err(),
            Some(RequestFailure::InvalidReply)
        );
    }

    /// The input the generator was sent, from the request `body`.
    fn generator_input(body: &Value) -> Value {
        serde_json::from_str(
            body["messages"][1]["content"]
                .as_str()
                .expect("the input is text"),
        )
        .expect("the input is JSON")
    }

    #[tokio::test]
    async fn the_generator_explains_each_criterion_asked_with_zero_retention_and_what_it_cost() {
        let (server, ai) = ai("/chat/completions", 200, EXPLANATION).await;

        let reply = ai
            .explain(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &[
                    (0, CriterionOutcome::NotMet),
                    (1, CriterionOutcome::NeedsReview),
                ],
            )
            .await
            .expect("the recorded reply decodes");

        assert_eq!(reply.content, paragraphs());
        assert_eq!(
            *reply.provenance,
            Provenance::new(
                stamp().request,
                Role::Generator,
                generator_model(),
                versions()
            )
            .with_generation(GenerationId::new(
                "gen-1792537737-d8T3jmrltBTKAIJQNBGU".to_owned()
            ))
            .with_reported_model(ModelId::new("anthropic/claude-sonnet-5.5".to_owned()))
            .with_usage(Usage {
                input_tokens: Some(3100),
                output_tokens: Some(430),
            })
            .with_cost(Cost::new(0.0105).expect("0.0105 is a cost"))
        );
        let (_, body) = received(&server).await;
        assert_eq!(body["model"], generator_model().as_str());
        assert_eq!(
            body["provider"],
            serde_json::json!({ "zdr": true, "data_collection": "deny" })
        );
        assert_eq!(body.get("tools"), None::<&Value>);
        let input = generator_input(&body);
        assert_eq!(input["reasoning"], REASONING, "the reasoning as saved");
        assert_eq!(input["criteria"]["criterion_1"]["outcome"], "not met");
        assert_eq!(input["criteria"]["criterion_2"]["outcome"], "needs review");
    }

    #[tokio::test]
    async fn the_generator_is_asked_about_the_criteria_given_alone() {
        let output = serde_json::json!({
            "paragraphs": [{
                "criterion": "criterion_2",
                "text": "Meeting it would name what the first charge may have done.",
                "citations": [{ "path": "src/payments/retry.rs", "first_line": 2, "last_line": 6 }],
            }],
        });
        let (server, ai) = ai("/chat/completions", 200, &with_output(EXPLANATION, &output)).await;

        let reply = ai
            .explain(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &[(1, CriterionOutcome::NeedsReview)],
            )
            .await
            .expect("the reply explains the criterion asked about");

        assert_eq!(
            reply
                .content
                .iter()
                .map(|paragraph| (paragraph.criterion, paragraph.outcome))
                .collect::<Vec<_>>(),
            [(1, CriterionOutcome::NeedsReview)]
        );
        let (_, body) = received(&server).await;
        let input = generator_input(&body);
        let criteria: Vec<&str> = input["criteria"]
            .as_object()
            .expect("the criteria are an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(criteria, ["criterion_2"]);
    }

    #[tokio::test]
    async fn an_explanation_that_leaves_out_a_criterion_asked_is_an_invalid_reply() {
        let (_server, ai) = ai("/chat/completions", 200, EXPLANATION).await;

        assert_eq!(
            ai.explain(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &[(0, CriterionOutcome::NotMet)],
            )
            .await
            .err(),
            Some(RequestFailure::InvalidReply),
            "the recording also explains criterion 2, which wasn't asked about"
        );
    }

    #[tokio::test]
    async fn nothing_to_explain_or_a_criterion_outside_the_rubric_is_an_invalid_request_that_sends_nothing()
     {
        let (server, ai) = ai("/chat/completions", 200, EXPLANATION).await;

        for criteria in [&[][..], &[(2, CriterionOutcome::NotMet)]] {
            assert_eq!(
                ai.explain(
                    stamp(),
                    &Acknowledged(REASONING),
                    &evidence(),
                    &question(),
                    criteria
                )
                .await
                .err(),
                Some(RequestFailure::InvalidRequest),
                "{criteria:?}"
            );
        }
        assert_eq!(
            server
                .received_requests()
                .await
                .map(|requests| requests.len()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn jev_checks_each_paragraph_with_the_model_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/systemone", 200, EXPLANATION_CHECK).await;

        let reply = ai
            .check_explanation(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &paragraphs(),
            )
            .await
            .expect("the recorded reply decodes");

        let answer = |value| Probability::new(value).expect("a probability");
        assert_eq!(
            reply.content,
            [
                ParagraphCheck {
                    supported: answer(0.71),
                    agrees: answer(0.85),
                },
                ParagraphCheck {
                    supported: answer(0.95),
                    agrees: answer(0.89),
                },
            ]
        );
        assert_eq!(
            without_exchanges(&reply.provenance),
            Provenance::new(stamp().request, Role::Jev, jev_model(), versions())
                .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(2062),
                    output_tokens: Some(90),
                })
                .with_cost(Cost::new(0.000_086_604).expect("0.000086604 is a cost"))
        );
        let (_, body) = received(&server).await;
        assert_kept_as_sent(reply.provenance.exchanges(), &body);
        assert_eq!(
            yes_no_answers(reply.provenance.exchanges()),
            [
                ("criterion_1_agrees", 0.85),
                ("criterion_1_supported", 0.71),
                ("criterion_2_agrees", 0.89),
                ("criterion_2_supported", 0.95),
            ]
        );
        assert_eq!(body["model"], jev_model().as_str());
        assert_eq!(body.get("provider"), None::<&Value>);
        let questions: Vec<&str> = body["questions"]
            .as_object()
            .expect("the questions are an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            questions,
            [
                "criterion_1_agrees",
                "criterion_1_supported",
                "criterion_2_agrees",
                "criterion_2_supported",
            ]
        );
    }

    #[tokio::test]
    async fn an_explanation_check_reply_that_answers_other_questions_is_an_invalid_reply() {
        let (_server, ai) = ai("/systemone", 200, REASONING_REPLY).await;

        assert_eq!(
            ai.check_explanation(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &paragraphs(),
            )
            .await
            .err(),
            Some(RequestFailure::InvalidReply)
        );
    }

    #[tokio::test]
    async fn a_refused_key_is_named_by_the_port() {
        let refused = include_str!("../fixtures/key-refused.json");
        let (_server, ai) = ai("/chat/completions", 401, refused).await;

        assert_eq!(
            ai.write_candidates(stamp(), &evidence(), &area(), two_per_area())
                .await
                .err(),
            Some(RequestFailure::KeyRefused)
        );
    }

    #[tokio::test]
    async fn a_request_openrouter_refuses_as_invalid_is_an_invalid_request() {
        let body = r#"{"error":{"code":400,"message":"typesafe/jev-9 is not a valid model ID"}}"#;
        let (_server, ai) = ai("/systemone", 400, body).await;

        assert_eq!(
            ai.check_question(
                stamp(),
                QuestionCheckCall::OptionRatings,
                &evidence(),
                &candidate()
            )
            .await
            .err(),
            Some(RequestFailure::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn a_question_check_reply_that_answers_other_questions_is_an_invalid_reply() {
        // Answers to the sample request's questions, none of them a check of
        // this candidate.
        let other = include_str!("../fixtures/jev-answers.json");
        let (_server, ai) = ai("/systemone", 200, other).await;

        assert_eq!(
            ai.check_question(
                stamp(),
                QuestionCheckCall::OptionRatings,
                &evidence(),
                &candidate()
            )
            .await
            .err(),
            Some(RequestFailure::InvalidReply)
        );
    }

    /// How each of the port's requests fails when `OpenRouter` answers
    /// `status` and `body`: the relevance gate, writing candidates and the
    /// question check, in that order.
    async fn failures(status: u16, body: &str) -> [Option<RequestFailure>; 3] {
        let (_gate, gate) = ai("/systemone", status, body).await;
        let (_write, write) = ai("/chat/completions", status, body).await;
        let (_check, check) = ai("/systemone", status, body).await;
        [
            gate.check_areas(stamp(), &evidence(), &[area()])
                .await
                .err(),
            write
                .write_candidates(stamp(), &evidence(), &area(), two_per_area())
                .await
                .err(),
            check
                .check_question(
                    stamp(),
                    QuestionCheckCall::OptionRatings,
                    &evidence(),
                    &candidate(),
                )
                .await
                .err(),
        ]
    }

    #[tokio::test]
    async fn each_failure_openrouter_reports_reaches_the_port_by_its_name() {
        use RequestFailure::{
            InvalidReply, InvalidRequest, KeyRefused, NoZeroRetentionEndpoint, OutOfCredits,
            ProviderUnavailable, RateLimited, TimedOut,
        };
        // Bodies that weren't recorded live follow `OpenRouter`'s documented
        // error shape: https://openrouter.ai/docs/api_reference/errors-and-debugging
        let server_error = |status| {
            format!(r#"{{"error":{{"code":{status},"message":"Internal Server Error"}}}}"#)
        };
        for (status, body, named) in [
            (
                401,
                include_str!("../fixtures/key-refused.json").to_owned(),
                [KeyRefused; 3],
            ),
            (
                402,
                r#"{"error":{"code":402,"message":"Insufficient credits"}}"#.to_owned(),
                [OutOfCredits; 3],
            ),
            (
                429,
                r#"{"error":{"code":429,"message":"Rate limit exceeded"}}"#.to_owned(),
                [RateLimited; 3],
            ),
            (500, server_error(500), [ProviderUnavailable; 3]),
            (502, server_error(502), [ProviderUnavailable; 3]),
            (503, server_error(503), [ProviderUnavailable; 3]),
            // Only the generator's requests ask for zero retention until #69,
            // so only for them is a refused route about it.
            (
                404,
                include_str!("../fixtures/generator-no-zero-retention.json").to_owned(),
                [
                    ProviderUnavailable,
                    NoZeroRetentionEndpoint,
                    ProviderUnavailable,
                ],
            ),
            (
                400,
                r#"{"error":{"code":400,"message":"not a valid model ID"}}"#.to_owned(),
                [InvalidRequest; 3],
            ),
            (
                403,
                r#"{"error":{"code":403,"message":"Input was flagged","metadata":{"reasons":["moderation"]}}}"#
                    .to_owned(),
                [InvalidRequest; 3],
            ),
            (
                408,
                r#"{"error":{"code":408,"message":"Request timed out"}}"#.to_owned(),
                [TimedOut; 3],
            ),
            (200, "not json".to_owned(), [InvalidReply; 3]),
        ] {
            assert_eq!(
                failures(status, &body).await,
                named.map(Some),
                "status {status}: {body}"
            );
        }
    }

    /// The real AI on a local server that answers `route` only after ten
    /// minutes, with the default time limits but for connecting, which can't
    /// fire first: a paused clock jumps to the next timer whenever the
    /// runtime waits on the socket.
    async fn slow(route: &str) -> (wiremock::MockServer, OpenRouterAi) {
        let server = server(route, 200, "{}", Duration::from_secs(600)).await;
        let timeouts = Timeouts {
            connect: Duration::from_secs(3600),
            ..Timeouts::default()
        };
        let ai = OpenRouterAi::new(connection(&server, timeouts), models());
        (server, ai)
    }

    #[tokio::test(start_paused = true)]
    async fn jev_s_requests_time_out_after_30_seconds() {
        let (_gate, gate) = slow("/systemone").await;
        let started = tokio::time::Instant::now();
        assert_eq!(
            gate.check_areas(stamp(), &evidence(), &[area()])
                .await
                .err(),
            Some(RequestFailure::TimedOut)
        );
        assert_eq!(started.elapsed(), Duration::from_secs(30), "the gate");

        let (_check, check) = slow("/systemone").await;
        let started = tokio::time::Instant::now();
        assert_eq!(
            check
                .check_question(
                    stamp(),
                    QuestionCheckCall::OptionRatings,
                    &evidence(),
                    &candidate(),
                )
                .await
                .err(),
            Some(RequestFailure::TimedOut)
        );
        assert_eq!(
            started.elapsed(),
            Duration::from_secs(30),
            "the question check"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_generator_s_requests_time_out_after_120_seconds() {
        let (_server, ai) = slow("/chat/completions").await;
        let started = tokio::time::Instant::now();

        assert_eq!(
            ai.write_candidates(stamp(), &evidence(), &area(), two_per_area())
                .await
                .err(),
            Some(RequestFailure::TimedOut)
        );
        assert_eq!(started.elapsed(), Duration::from_secs(120));
    }

    #[test]
    fn every_failure_keeps_its_name_across_the_port() {
        for (error, failure) in [
            (AiError::NoKey, RequestFailure::NoKey),
            (AiError::KeyRefused, RequestFailure::KeyRefused),
            (AiError::OutOfCredits, RequestFailure::OutOfCredits),
            (AiError::RateLimited, RequestFailure::RateLimited),
            (
                AiError::ProviderUnavailable { status: Some(503) },
                RequestFailure::ProviderUnavailable,
            ),
            (
                AiError::ProviderUnavailable { status: None },
                RequestFailure::ProviderUnavailable,
            ),
            (
                AiError::NoZeroRetentionEndpoint {
                    model: "cohere/command-r7b-12-2024".to_owned(),
                },
                RequestFailure::NoZeroRetentionEndpoint,
            ),
            (AiError::TimedOut, RequestFailure::TimedOut),
            (AiError::InvalidReply, RequestFailure::InvalidReply),
            (
                AiError::InvalidRequest { status: Some(403) },
                RequestFailure::InvalidRequest,
            ),
            (
                AiError::InvalidRequest { status: None },
                RequestFailure::InvalidRequest,
            ),
            (AiError::Cancelled, RequestFailure::Cancelled),
        ] {
            assert_eq!(RequestFailure::from(error.clone()), failure, "{error:?}");
        }
    }

    /// Log lines, as the app's log file gets them.
    #[derive(Debug, Clone, Default)]
    struct Log(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Log {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("no test thread panics holding the log")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Log {
        fn text(&self) -> String {
            String::from_utf8_lossy(
                &self
                    .0
                    .lock()
                    .expect("no test thread panics holding the log"),
            )
            .into_owned()
        }
    }

    #[tokio::test]
    async fn a_request_logs_its_models_and_cost_and_nothing_it_sends_or_gets_back() {
        let log = Log::default();
        let writer = log.clone();
        // Every target, Rig's included, at the level the app logs at, in
        // plain text as the app writes it.
        let _logging = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(move || writer.clone())
                .with_max_level(tracing::Level::INFO)
                .with_ansi(false)
                .finish(),
        );

        let (_generator, generator) = ai("/chat/completions", 200, CANDIDATES).await;
        generator
            .write_candidates(stamp(), &evidence(), &area(), two_per_area())
            .await
            .expect("the recorded reply decodes");
        let (_jev, jev) = ai(
            "/systemone",
            200,
            recorded_check(QuestionCheckCall::OptionRatings).1,
        )
        .await;
        jev.check_question(
            stamp(),
            QuestionCheckCall::OptionRatings,
            &evidence(),
            &candidate(),
        )
        .await
        .expect("the recorded reply decodes");
        let (_gate, gate) = ai("/systemone", 200, GATE).await;
        gate.check_areas(stamp(), &evidence(), &[area(), observability()])
            .await
            .expect("the recorded reply decodes");
        let explained = paragraphs();
        let (_explaining, explaining) = ai("/chat/completions", 200, EXPLANATION).await;
        explaining
            .explain(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &[
                    (0, CriterionOutcome::NotMet),
                    (1, CriterionOutcome::NeedsReview),
                ],
            )
            .await
            .expect("the recorded reply decodes");
        let (_checking, checking) = ai("/systemone", 200, EXPLANATION_CHECK).await;
        checking
            .check_explanation(
                stamp(),
                &Acknowledged(REASONING),
                &evidence(),
                &question(),
                &explained,
            )
            .await
            .expect("the recorded reply decodes");

        let log = log.text();
        assert!(log.contains("OpenRouter replied"), "{log}");
        assert!(
            log.contains("cost_usd=0.036766"),
            "the generator's cost: {log}"
        );
        assert!(log.contains("cost_usd=6.5646e-5"), "Jev's cost: {log}");
        assert!(log.contains("cost_usd=5.1828e-5"), "the gate's cost: {log}");
        assert!(log.contains("request=1"), "Whetstone's identity: {log}");
        assert!(
            log.contains("gen-1792574742-6mTYp6eLYi65k1QVjIP4"),
            "OpenRouter's generation ID: {log}"
        );
        for never in [
            // Jev's answers: an option rating, the depth, the gate's answer
            // about an area, and an explanation check's.
            "0.99",
            "1.47",
            "0.89",
            "0.71",
            // The key.
            crate::replay::TEST_KEY,
            // A base prompt, the area prompt, and the evidence.
            "You write candidate questions for Whetstone",
            "Given the evidence",
            "Focus on the rules the change encodes",
            // The gate's question about an area.
            "worth answering about the area",
            "Reuse the charge's key when retrying after a timeout",
            "let key = original.key.clone();",
            // A candidate sent to Jev, and one the generator wrote.
            "Why does the retry in `charge_with_retry`",
            "A charge request times out, but the provider had already recorded it",
            "It confirms that two charge requests are made",
            // The learner's reasoning, and the explanation of it.
            REASONING,
            &explained[0].text,
            &explained[1].text,
        ] {
            assert!(!log.contains(never), "{never:?} reached the log:\n{log}");
        }
    }
}
