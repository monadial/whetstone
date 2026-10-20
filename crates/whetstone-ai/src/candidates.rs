//! Writing candidates: one generator call per area, which writes candidate
//! questions about the change from the evidence alone.
//!
//! [`candidates_request`] builds the call, and its reply's structured output
//! is a [`CandidatesOutput`], which [`CandidatesOutput::into_written`] turns
//! into whetstone-core's [`WrittenCandidate`]s as written, for the plain
//! checks to decide.

use std::fmt;
use std::num::NonZeroUsize;

use schemars::{JsonSchema, Schema, json_schema};
use serde::Deserialize;
use serde_json::Value;
use whetstone_core::{
    Area, Evidence, ModelId, QuestionType, WrittenCandidate, WrittenCitation, WrittenCriterion,
    WrittenOption,
};

use crate::evidence::evidence_as_sent;
use crate::generator::GeneratorRequest;
use crate::prompts::{AREA, EVIDENCE, WRITING, fill};

/// The generator request that writes `count` candidate questions for `area`
/// with `model`.
///
/// Its preamble is Whetstone's base prompts for writing, then `area`'s name,
/// description and area prompt, which add focus and never replace them. Its
/// input is the evidence as sent, with every line numbered for citing.
/// Area prompts are never part of the evidence.
#[must_use]
pub fn candidates_request(
    model: &ModelId,
    evidence: &Evidence,
    area: &Area,
    count: NonZeroUsize,
) -> GeneratorRequest {
    let preamble = [
        fill(WRITING, &[("count", &count.to_string())]),
        EVIDENCE.to_owned(),
        fill(
            AREA,
            &[
                ("name", area.name()),
                ("description", area.description()),
                ("prompt", area.prompt()),
            ],
        ),
    ]
    .map(|part| part.trim_end().to_owned())
    .join("\n\n");
    GeneratorRequest {
        model: model.as_str().to_owned(),
        preamble,
        input: evidence_as_sent(evidence).to_string(),
    }
}

/// The candidates the generator wrote for one area: the structured output a
/// [`candidates_request`] asks for.
///
/// Its JSON schema suits strict structured output: every field is required
/// and no other is allowed. It carries no descriptions, so that every word
/// the generator is told comes from the versioned base prompts. A part a
/// candidate leaves out decodes as empty, which the plain checks drop it
/// for, rather than failing the other candidates.
#[derive(Deserialize, JsonSchema)]
#[schemars(transform = strict)]
pub struct CandidatesOutput {
    candidates: Vec<CandidateOutput>,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(transform = strict)]
struct CandidateOutput {
    stem: String,
    options: Vec<OptionOutput>,
    answer_key: String,
    why_it_matters: String,
    rubric: Vec<CriterionOutput>,
    hint: String,
    citations: Vec<CitationOutput>,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(transform = strict)]
struct OptionOutput {
    letter: String,
    text: String,
    rationale: String,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(transform = strict)]
struct CriterionOutput {
    statement: String,
    met: String,
    not_met: String,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(transform = strict)]
struct CitationOutput {
    path: String,
    #[schemars(schema_with = "line_number")]
    first_line: usize,
    #[schemars(schema_with = "line_number")]
    last_line: usize,
}

impl CandidatesOutput {
    /// The candidates as the generator wrote them, in its order, for
    /// whetstone-core's plain checks: each a single choice, with a part it
    /// left out as empty text, or as line 0.
    #[must_use]
    pub fn into_written(self) -> Vec<WrittenCandidate> {
        self.candidates
            .into_iter()
            .map(|candidate| WrittenCandidate {
                question_type: QuestionType::SingleChoice,
                stem: candidate.stem,
                options: candidate
                    .options
                    .into_iter()
                    .map(|option| WrittenOption {
                        letter: option.letter,
                        text: option.text,
                        rationale: option.rationale,
                    })
                    .collect(),
                answer_key: candidate.answer_key,
                why_it_matters: candidate.why_it_matters,
                rubric: candidate
                    .rubric
                    .into_iter()
                    .map(|criterion| WrittenCriterion {
                        statement: criterion.statement,
                        met: criterion.met,
                        not_met: criterion.not_met,
                    })
                    .collect(),
                hint: candidate.hint,
                citations: candidate
                    .citations
                    .into_iter()
                    .map(|citation| WrittenCitation {
                        path: citation.path,
                        first_line: citation.first_line,
                        last_line: citation.last_line,
                    })
                    .collect(),
            })
            .collect()
    }
}

/// Shows how many candidates it holds, never their text.
impl fmt::Debug for CandidatesOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CandidatesOutput")
            .field("candidates", &self.candidates.len())
            .finish()
    }
}

/// A line number: an integer, with no format or bounds, which not every
/// provider's strict structured output takes.
fn line_number(_: &mut schemars::SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer" })
}

/// Takes out what strict structured output doesn't need: the descriptions
/// that doc comments become, which would be prompt text outside the base
/// prompts, and the defaults that `#[serde(default)]` adds.
fn strict(schema: &mut Schema) {
    schema.remove("description");
    if let Some(Value::Object(properties)) = schema.get_mut("properties") {
        for property in properties.values_mut() {
            if let Value::Object(property) = property {
                property.remove("description");
                property.remove("default");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use whetstone_core::{DropReason, Part, plain_checks};

    use super::*;
    use crate::AiError;
    use crate::samples::{area, evidence, generated, generator_body, generator_model, with_output};

    const RECORDED: &str = include_str!("../fixtures/candidates.json");

    fn request() -> GeneratorRequest {
        candidates_request(
            &generator_model(),
            &evidence(),
            &area(),
            NonZeroUsize::new(2).expect("2 is not 0"),
        )
    }

    #[tokio::test]
    async fn a_candidates_request_sends_the_base_prompts_and_the_area_prompt_with_the_evidence() {
        let (_, body) = generated::<CandidatesOutput>(&request(), "{}").await;

        insta::assert_snapshot!(generator_body(body));
    }

    #[tokio::test]
    async fn recorded_candidates_decode_as_written_and_pass_the_plain_checks() {
        let (reply, _) = generated::<CandidatesOutput>(&request(), RECORDED).await;
        let written = reply
            .expect("the recorded output decodes")
            .output
            .into_written();

        assert_eq!(written.len(), 2);
        let first = &written[0];
        assert_eq!(
            first.stem,
            "Before this change, the retry after a `Timeout` generated a fresh `OperationKey`. \
             Given how the provider treats keys, what could go wrong with that behaviour that \
             the change prevents?"
        );
        assert_eq!(
            first
                .options
                .iter()
                .map(|option| option.letter.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
        assert_eq!(first.answer_key, "A");
        assert_eq!(first.rubric.len(), 2);
        assert_eq!(
            first.citations,
            [
                WrittenCitation {
                    path: "src/payments/client.rs".to_owned(),
                    first_line: 12,
                    last_line: 16,
                },
                WrittenCitation {
                    path: "src/payments/retry.rs".to_owned(),
                    first_line: 3,
                    last_line: 6,
                },
            ]
        );
        assert_eq!(written[1].answer_key, "B");
        let checked = plain_checks(area().id(), written, &evidence());
        assert_eq!(checked.passed().len(), 2);
    }

    #[tokio::test]
    async fn a_part_left_out_decodes_as_empty_and_the_plain_checks_drop_its_candidate() {
        let output = json!({
            "candidates": [{
                "stem": "Why does the retry reuse the key?",
                "options": [
                    { "letter": "A", "text": "One", "rationale": "Right" },
                    { "letter": "B", "text": "Two", "rationale": "Wrong" },
                    { "letter": "C", "text": "Three", "rationale": "Wrong" },
                ],
                "answer_key": "A",
                "why_it_matters": "A retry mustn't charge twice.",
                "rubric": [{ "statement": "Covers it.", "met": "Does.", "not_met": "Doesn't." }],
                "citations": [{ "path": "src/payments/retry.rs", "first_line": 3, "last_line": 6 }],
            }],
        });

        let (reply, _) =
            generated::<CandidatesOutput>(&request(), &with_output(RECORDED, &output)).await;
        let written = reply.expect("the output decodes").output.into_written();

        assert_eq!(written[0].hint, "");
        assert_eq!(
            plain_checks(area().id(), written, &evidence()).dropped(),
            [DropReason::Missing(Part::Hint)]
        );
    }

    #[tokio::test]
    async fn output_without_candidates_is_an_invalid_reply() {
        let (reply, _) =
            generated::<CandidatesOutput>(&request(), &with_output(RECORDED, &json!({}))).await;

        assert_eq!(reply.err(), Some(AiError::InvalidReply));
    }
}
