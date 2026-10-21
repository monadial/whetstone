//! Reasoning: one Jev call per submitted response, which asks for each
//! criterion of the question's rubric whether the learner's reasoning meets
//! it.
//!
//! Jev never sees which option the learner chose, or the answer key, so
//! the reasoning outcome is independent of the choice. [`reasoning_request`]
//! takes the question as a [`QuestionWithoutKey`] and the reasoning as
//! text, so it has neither to send.

use std::fmt;

use serde_json::{Map, Value, json};
use whetstone_core::{Criterion, Evidence, ModelId, OptionLetter, Probability, Question};

use crate::answers::Answers;
use crate::evidence::evidence_as_sent;
use crate::failure::AiError;
use crate::jev::{JevReply, JevRequest};
use crate::prompts::REASONING;
use crate::question_check::criterion_name;

/// What a reasoning request may send of a question: its stem, its options'
/// text and its rubric. It holds no answer key and no rationales, which
/// would name the answer key, and nothing of the learner's choice.
#[derive(Clone, Copy)]
pub struct QuestionWithoutKey<'a> {
    stem: &'a str,
    options: [&'a str; 3],
    rubric: &'a [Criterion],
}

impl<'a> From<&'a Question> for QuestionWithoutKey<'a> {
    fn from(question: &'a Question) -> Self {
        Self {
            stem: question.stem(),
            options: OptionLetter::ALL.map(|letter| question.option(letter)),
            rubric: question.rubric(),
        }
    }
}

impl<'a> QuestionWithoutKey<'a> {
    /// The question's stem.
    pub(crate) fn stem(&self) -> &'a str {
        self.stem
    }

    /// Options A, B and C's text, in that order.
    pub(crate) fn options(&self) -> [&'a str; 3] {
        self.options
    }

    /// Its rubric's criteria, in order.
    pub(crate) fn rubric(&self) -> &'a [Criterion] {
        self.rubric
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for QuestionWithoutKey<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuestionWithoutKey")
            .field("stem_bytes", &self.stem.len())
            .field("options_bytes", &self.options.map(str::len))
            .field("rubric", &self.rubric)
            .finish()
    }
}

/// The Jev request that assesses `reasoning`, written for `question`, with
/// `model`: one yes/no question per criterion of the rubric, named
/// `criterion_1` and on, from the base prompts' template filled with the
/// criterion's statement, with what meeting it and not meeting it look like
/// as what yes and no mean.
///
/// Its state is the evidence as sent, as `evidence`; the question's stem and
/// options, as `question`; and the reasoning, as `reasoning`. It never holds
/// the chosen option or the answer key.
#[must_use]
pub fn reasoning_request(
    model: &ModelId,
    evidence: &Evidence,
    question: QuestionWithoutKey<'_>,
    reasoning: &str,
) -> JevRequest {
    let options: Map<String, Value> = OptionLetter::ALL
        .into_iter()
        .zip(question.options())
        .map(|(letter, text)| (letter.to_string(), json!(text)))
        .collect();
    JevRequest {
        model: model.as_str().to_owned(),
        state: json!({
            "evidence": evidence_as_sent(evidence),
            "question": { "stem": question.stem(), "options": options },
            "reasoning": reasoning,
        }),
        questions: question
            .rubric
            .iter()
            .enumerate()
            .map(|(index, criterion)| {
                (
                    criterion_name(index),
                    REASONING.criterion.question(
                        &[("statement", criterion.statement())],
                        criterion.met(),
                        criterion.not_met(),
                    ),
                )
            })
            .collect(),
    }
}

/// Jev's answer for each criterion of `rubric`, from `reply`, in rubric
/// order: the probability that the reasoning meets it, exactly as returned,
/// for the assessment policy to turn into met, not met or needs review.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, not a probability, or to a question that wasn't asked.
pub fn reasoning_answers(
    rubric: &[Criterion],
    reply: &JevReply,
) -> Result<Vec<Probability>, AiError> {
    let mut answers = Answers::of(reply);
    let answered = (0..rubric.len())
        .map(|index| answers.yes_no(&criterion_name(index)))
        .collect::<Result<Vec<_>, AiError>>()?;
    answers.done()?;
    Ok(answered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::JevAnswer;
    use crate::samples::{REASONING, evaluated, evidence, jev_model, pretty, question};

    const RECORDED: &str = include_str!("../fixtures/reasoning.json");

    fn request() -> JevRequest {
        reasoning_request(
            &jev_model(),
            &evidence(),
            QuestionWithoutKey::from(&question()),
            REASONING,
        )
    }

    async fn recorded() -> JevReply {
        evaluated(&request(), RECORDED)
            .await
            .0
            .expect("the recorded reply decodes")
    }

    #[tokio::test]
    async fn a_reasoning_request_sends_the_evidence_question_and_reasoning_with_a_question_per_criterion()
     {
        let (_, body) = evaluated(&request(), "{}").await;

        insta::assert_snapshot!(pretty(&body));
    }

    #[tokio::test]
    async fn a_reasoning_request_holds_neither_the_chosen_option_nor_the_answer_key() {
        let (_, body) = evaluated(&request(), "{}").await;
        let question = question();

        let state = &body["state"];
        assert_eq!(
            state
                .as_object()
                .map(|state| state.keys().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["evidence", "question", "reasoning"])
        );
        assert_eq!(
            state["question"],
            json!({
                "stem": question.stem(),
                "options": {
                    "A": question.option(OptionLetter::A),
                    "B": question.option(OptionLetter::B),
                    "C": question.option(OptionLetter::C),
                },
            })
        );
        // The rationales say which option is correct.
        let body = body.to_string();
        for letter in OptionLetter::ALL {
            assert!(!body.contains(question.rationale(letter)));
        }
        assert!(!body.contains("answer_key"));
    }

    #[tokio::test]
    async fn recorded_answers_decode_as_a_probability_per_criterion_in_rubric_order() {
        let answers = reasoning_answers(question().rubric(), &recorded().await)
            .expect("every criterion is answered");

        assert_eq!(
            answers,
            [0.75, 0.96].map(|answer| Probability::new(answer).expect("a probability"))
        );
    }

    #[tokio::test]
    async fn a_missing_answer_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.remove("criterion_2");

        assert_eq!(
            reasoning_answers(question().rubric(), &reply),
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn an_answer_of_another_kind_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.insert(
            "criterion_1".to_owned(),
            JevAnswer::Score {
                probabilities: vec![0.5, 0.5],
                expected: 0.5,
            },
        );

        assert_eq!(
            reasoning_answers(question().rubric(), &reply),
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn an_answer_to_a_criterion_not_in_the_rubric_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply
            .answers
            .insert("criterion_3".to_owned(), JevAnswer::YesNo { yes: 0.5 });

        assert_eq!(
            reasoning_answers(question().rubric(), &reply),
            Err(AiError::InvalidReply)
        );
    }
}
