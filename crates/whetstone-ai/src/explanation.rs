//! The explanation: when the reasoning didn't meet a criterion, or Jev
//! couldn't tell, one generator call writes a paragraph for each such
//! criterion, and one Jev call checks every paragraph before the learner
//! sees it.
//!
//! [`explanation_request`] builds the generator call, whose structured
//! output is an [`ExplanationOutput`], and [`explanation_paragraphs`] reads
//! it into [`WrittenParagraph`]s. [`explanation_check_request`] and
//! [`explanation_check_answers`] check them. The outcomes always come from
//! Jev's assessment of the reasoning, never from the explanation's wording.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use whetstone_core::{
    CriterionOutcome, Evidence, ModelId, OptionLetter, Probability, Question, WrittenCitation,
};

use crate::answers::Answers;
use crate::evidence::evidence_as_sent;
use crate::failure::AiError;
use crate::generator::GeneratorRequest;
use crate::jev::{JevReply, JevRequest};
use crate::output::{CitationOutput, strict};
use crate::prompts::{EVIDENCE, EXPLANATION, EXPLANATION_CHECK};
use crate::question_check::criterion_name;

/// The generator request that explains, with `model`, what `reasoning`
/// missed for `question`, whose criteria's `outcomes` are given in rubric
/// order: one paragraph for each criterion that is not met or needs review,
/// with citations. For one that needs review, its instructions are to
/// describe what meeting the criterion would look like, without claiming
/// the learner missed it.
///
/// Its preamble is Whetstone's base prompts for explaining, and its input
/// is the evidence as sent, the question with its answer key, the reasoning
/// and the criteria to explain, each named `criterion_1` and on by its place
/// in the rubric, with its outcome. It is `None` when every criterion is
/// met: there is nothing to explain.
#[must_use]
pub fn explanation_request(
    model: &ModelId,
    evidence: &Evidence,
    question: &Question,
    reasoning: &str,
    outcomes: &[CriterionOutcome],
) -> Option<GeneratorRequest> {
    let criteria: Map<String, Value> = question
        .rubric()
        .iter()
        .zip(outcomes)
        .enumerate()
        .filter_map(|(index, (criterion, &outcome))| {
            Some((
                criterion_name(index),
                json!({
                    "statement": criterion.statement(),
                    "met": criterion.met(),
                    "not_met": criterion.not_met(),
                    "outcome": explained(outcome).then(|| read(outcome))?,
                }),
            ))
        })
        .collect();
    if criteria.is_empty() {
        return None;
    }
    let mut asked = question_state(question);
    asked["why_it_matters"] = json!(question.why_it_matters());
    let input = json!({
        "evidence": evidence_as_sent(evidence),
        "question": asked,
        "reasoning": reasoning,
        "criteria": criteria,
    });
    Some(GeneratorRequest {
        model: model.as_str().to_owned(),
        preamble: [EXPLANATION, EVIDENCE].map(str::trim_end).join("\n\n"),
        input: input.to_string(),
    })
}

/// The paragraphs the generator wrote: the structured output an
/// [`explanation_request`] asks for.
///
/// Like [`CandidatesOutput`](crate::CandidatesOutput), its JSON schema
/// suits strict structured output and carries no descriptions.
#[derive(Deserialize, JsonSchema)]
#[schemars(transform = strict)]
pub struct ExplanationOutput {
    paragraphs: Vec<ParagraphOutput>,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(transform = strict)]
struct ParagraphOutput {
    criterion: String,
    text: String,
    citations: Vec<CitationOutput>,
}

/// One paragraph of an explanation, as the generator wrote it: the
/// criterion it explains and that criterion's outcome, its text, and its
/// citations, which plain code still has to check against the evidence.
#[derive(Clone, PartialEq, Eq)]
pub struct WrittenParagraph {
    /// The criterion it explains: its place in the rubric, counting from 0.
    pub criterion: usize,
    /// The outcome it explains: not met, or needs review.
    pub outcome: CriterionOutcome,
    /// What it says.
    pub text: String,
    /// The evidence it rests on.
    pub citations: Vec<WrittenCitation>,
}

/// What Jev answered about one paragraph in the explanation check, each the
/// probability of yes, exactly as returned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphCheck {
    /// That the paragraph is supported by the evidence.
    pub supported: Probability,
    /// That the paragraph agrees with its criterion's outcome.
    pub agrees: Probability,
}

/// The paragraphs in `output`, in rubric order, for the criteria whose
/// `outcomes`, in rubric order, the request asked to explain.
///
/// Fails with [`AiError::InvalidReply`] unless there is exactly one
/// paragraph for each criterion that is not met or needs review, and none
/// for another.
pub fn explanation_paragraphs(
    outcomes: &[CriterionOutcome],
    output: ExplanationOutput,
) -> Result<Vec<WrittenParagraph>, AiError> {
    let mut asked: BTreeMap<String, (usize, CriterionOutcome)> = outcomes
        .iter()
        .enumerate()
        .filter(|&(_, &outcome)| explained(outcome))
        .map(|(index, &outcome)| (criterion_name(index), (index, outcome)))
        .collect();
    let mut paragraphs = output
        .paragraphs
        .into_iter()
        .map(|paragraph| {
            let (criterion, outcome) = asked
                .remove(&paragraph.criterion)
                .ok_or(AiError::InvalidReply)?;
            Ok(WrittenParagraph {
                criterion,
                outcome,
                text: paragraph.text,
                citations: paragraph
                    .citations
                    .into_iter()
                    .map(CitationOutput::into_written)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, AiError>>()?;
    if !asked.is_empty() {
        return Err(AiError::InvalidReply);
    }
    paragraphs.sort_by_key(|paragraph| paragraph.criterion);
    Ok(paragraphs)
}

/// The Jev request that checks `paragraphs`, explaining criteria of
/// `question`, with `model`: two yes/no questions per paragraph, from the
/// base prompts, named after its criterion: `criterion_2_supported`, that
/// it is supported by the evidence, and `criterion_2_agrees`, that it agrees
/// with its criterion's outcome.
///
/// Its state is the evidence as sent, as `evidence`; the question's stem,
/// options and answer key, as `question`; and each paragraph under its
/// criterion's name, with the criterion, its outcome and the paragraph's
/// citations, as `explanations`. It holds no reasoning.
#[must_use]
pub fn explanation_check_request(
    model: &ModelId,
    evidence: &Evidence,
    question: &Question,
    paragraphs: &[WrittenParagraph],
) -> JevRequest {
    let explanations: Map<String, Value> = paragraphs
        .iter()
        .map(|paragraph| {
            let mut explanation = question.rubric().get(paragraph.criterion).map_or_else(
                || json!({}),
                |criterion| {
                    json!({
                        "statement": criterion.statement(),
                        "met": criterion.met(),
                        "not_met": criterion.not_met(),
                    })
                },
            );
            explanation["outcome"] = json!(read(paragraph.outcome));
            explanation["explanation"] = json!(paragraph.text);
            explanation["citations"] = paragraph
                .citations
                .iter()
                .map(|citation| {
                    json!({
                        "path": citation.path,
                        "first_line": citation.first_line,
                        "last_line": citation.last_line,
                    })
                })
                .collect();
            (criterion_name(paragraph.criterion), explanation)
        })
        .collect();
    JevRequest {
        model: model.as_str().to_owned(),
        state: json!({
            "evidence": evidence_as_sent(evidence),
            "question": question_state(question),
            "explanations": explanations,
        }),
        questions: paragraphs
            .iter()
            .flat_map(|paragraph| {
                let criterion = criterion_name(paragraph.criterion);
                let values = [("criterion", criterion.as_str())];
                [
                    (
                        format!("{criterion}_supported"),
                        EXPLANATION_CHECK.supported.question(&values),
                    ),
                    (
                        format!("{criterion}_agrees"),
                        EXPLANATION_CHECK.agrees.question(&values),
                    ),
                ]
            })
            .collect(),
    }
}

/// Jev's answers about each of `paragraphs`, from `reply`, in their order,
/// exactly as returned.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, not a probability, or to a question that wasn't asked.
pub fn explanation_check_answers(
    paragraphs: &[WrittenParagraph],
    reply: &JevReply,
) -> Result<Vec<ParagraphCheck>, AiError> {
    let mut answers = Answers::of(reply);
    let checks = paragraphs
        .iter()
        .map(|paragraph| {
            let criterion = criterion_name(paragraph.criterion);
            Ok(ParagraphCheck {
                supported: answers.yes_no(&format!("{criterion}_supported"))?,
                agrees: answers.yes_no(&format!("{criterion}_agrees"))?,
            })
        })
        .collect::<Result<Vec<_>, AiError>>()?;
    answers.done()?;
    Ok(checks)
}

/// Whether a criterion with `outcome` is explained: it is not met, or needs
/// review.
fn explained(outcome: CriterionOutcome) -> bool {
    matches!(
        outcome,
        CriterionOutcome::NotMet | CriterionOutcome::NeedsReview
    )
}

/// How `outcome` reads to the generator and Jev.
fn read(outcome: CriterionOutcome) -> &'static str {
    match outcome {
        CriterionOutcome::Met => "met",
        CriterionOutcome::NotMet => "not met",
        CriterionOutcome::NeedsReview => "needs review",
    }
}

/// `question`'s stem, options and answer key.
fn question_state(question: &Question) -> Value {
    let options: Map<String, Value> = OptionLetter::ALL
        .into_iter()
        .map(|letter| (letter.to_string(), json!(question.option(letter))))
        .collect();
    json!({
        "stem": question.stem(),
        "options": options,
        "answer_key": question.answer_key().to_string(),
    })
}

/// Shows how many paragraphs it holds, never their text.
impl fmt::Debug for ExplanationOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExplanationOutput")
            .field("paragraphs", &self.paragraphs.len())
            .finish()
    }
}

/// Shows its criterion, outcome and citations, and its text's length, never
/// the text.
impl fmt::Debug for WrittenParagraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrittenParagraph")
            .field("criterion", &self.criterion)
            .field("outcome", &self.outcome)
            .field("text_bytes", &self.text.len())
            .field("citations", &self.citations)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::JevAnswer;
    use crate::samples::{
        OUTCOMES, REASONING, evaluated, evidence, generated, generator_body, generator_model,
        jev_model, paragraphs, pretty, question, with_output,
    };

    const RECORDED: &str = include_str!("../fixtures/explanation.json");
    const RECORDED_CHECK: &str = include_str!("../fixtures/explanation-check.json");

    fn request() -> GeneratorRequest {
        explanation_request(
            &generator_model(),
            &evidence(),
            &question(),
            REASONING,
            &OUTCOMES,
        )
        .expect("a criterion needs explaining")
    }

    async fn decoded(reply: &str) -> Result<Vec<WrittenParagraph>, AiError> {
        let (reply, _) = generated::<ExplanationOutput>(&request(), reply).await;
        explanation_paragraphs(&OUTCOMES, reply?.output)
    }

    fn check_request() -> JevRequest {
        explanation_check_request(&jev_model(), &evidence(), &question(), &paragraphs())
    }

    async fn recorded_check() -> JevReply {
        evaluated(&check_request(), RECORDED_CHECK)
            .await
            .0
            .expect("the recorded reply decodes")
    }

    #[tokio::test]
    async fn an_explanation_request_sends_the_question_reasoning_and_each_criterion_to_explain() {
        let (_, body) = generated::<ExplanationOutput>(&request(), "{}").await;

        insta::assert_snapshot!(generator_body(body));
    }

    #[test]
    fn there_is_no_explanation_request_when_every_criterion_is_met() {
        let request = explanation_request(
            &generator_model(),
            &evidence(),
            &question(),
            REASONING,
            &[CriterionOutcome::Met; 2],
        );

        assert_eq!(request, None);
    }

    #[test]
    fn only_criteria_not_met_or_needing_review_are_explained() {
        let request = explanation_request(
            &generator_model(),
            &evidence(),
            &question(),
            REASONING,
            &[CriterionOutcome::Met, CriterionOutcome::NeedsReview],
        )
        .expect("a criterion needs review");
        let input: Value = serde_json::from_str(&request.input).expect("the input is JSON");

        assert_eq!(
            input["criteria"]
                .as_object()
                .map(|criteria| criteria.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["criterion_2"])
        );
        assert_eq!(input["criteria"]["criterion_2"]["outcome"], "needs review");
    }

    #[tokio::test]
    async fn a_recorded_explanation_decodes_as_a_paragraph_per_criterion_in_rubric_order() {
        let paragraphs = decoded(RECORDED).await.expect("the recording decodes");

        assert_eq!(
            paragraphs
                .iter()
                .map(|paragraph| (paragraph.criterion, paragraph.outcome))
                .collect::<Vec<_>>(),
            [
                (0, CriterionOutcome::NotMet),
                (1, CriterionOutcome::NeedsReview)
            ]
        );
        assert!(
            paragraphs
                .iter()
                .all(|paragraph| !paragraph.text.is_empty())
        );
        assert!(
            paragraphs
                .iter()
                .all(|paragraph| !paragraph.citations.is_empty())
        );
    }

    #[tokio::test]
    async fn a_paragraph_for_a_criterion_not_asked_about_is_an_invalid_reply() {
        let output = json!({
            "paragraphs": [
                { "criterion": "criterion_1", "text": "Missed.", "citations": [] },
                { "criterion": "criterion_2", "text": "Would cover.", "citations": [] },
                { "criterion": "criterion_3", "text": "Extra.", "citations": [] },
            ],
        });

        assert_eq!(
            decoded(&with_output(RECORDED, &output)).await,
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn a_missing_or_repeated_paragraph_is_an_invalid_reply() {
        for paragraphs in [
            json!([{ "criterion": "criterion_1", "text": "Missed.", "citations": [] }]),
            json!([
                { "criterion": "criterion_1", "text": "Missed.", "citations": [] },
                { "criterion": "criterion_1", "text": "Again.", "citations": [] },
                { "criterion": "criterion_2", "text": "Would cover.", "citations": [] },
            ]),
        ] {
            let output = json!({ "paragraphs": paragraphs });

            assert_eq!(
                decoded(&with_output(RECORDED, &output)).await,
                Err(AiError::InvalidReply)
            );
        }
    }

    #[tokio::test]
    async fn an_explanation_check_sends_each_paragraph_with_two_questions() {
        let (_, body) = evaluated(&check_request(), "{}").await;

        insta::assert_snapshot!(pretty(&body));
    }

    #[tokio::test]
    async fn recorded_explanation_check_answers_decode_per_paragraph() {
        let checks = explanation_check_answers(&paragraphs(), &recorded_check().await)
            .expect("every paragraph is answered");

        let answer = |value| Probability::new(value).expect("a probability");
        assert_eq!(
            checks,
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
    }

    #[tokio::test]
    async fn a_missing_check_answer_is_an_invalid_reply() {
        let mut reply = recorded_check().await;
        reply.answers.remove("criterion_2_agrees");

        assert_eq!(
            explanation_check_answers(&paragraphs(), &reply),
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn a_check_answer_of_another_kind_is_an_invalid_reply() {
        let mut reply = recorded_check().await;
        reply.answers.insert(
            "criterion_1_supported".to_owned(),
            JevAnswer::Score {
                probabilities: vec![0.5, 0.5],
                expected: 0.5,
            },
        );

        assert_eq!(
            explanation_check_answers(&paragraphs(), &reply),
            Err(AiError::InvalidReply)
        );
    }
}
