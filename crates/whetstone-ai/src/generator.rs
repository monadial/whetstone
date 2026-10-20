//! Generator calls: one Rig agent per call, on `OpenRouter`'s chat
//! completions, whose structured output decodes into the caller's type.
//!
//! The agent has a preamble and no tools, so it can read and send nothing
//! beyond its input. Every request requires zero-data retention and denies
//! data collection, so `OpenRouter` routes it only to endpoints that keep no
//! prompts, or refuses it.

use rig::AgentBuilder;
use rig::completion::{PromptError, StructuredOutputError};
use rig::providers::openai::OpenAIConfig;
use rig::providers::openai::wire::OPENROUTER;
use rig::providers::openrouter::extension::{
    DataCollection, OpenRouterOptions, ProviderPreferences,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

use crate::connection::{OpenRouter, bounded};
use crate::failure::{AiError, Routing};
use crate::usage::Usage;

/// One generator call: the model, the preamble that instructs it and the
/// input it works from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorRequest {
    /// The `OpenRouter` model, such as `anthropic/claude-sonnet-5.5`.
    pub model: String,
    /// The system instructions.
    pub preamble: String,
    /// The user message: what the model works from.
    pub input: String,
}

/// The generator's reply: its output decoded as `T`, with the model that
/// wrote it and what the call used.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratorReply<T> {
    /// The structured output.
    pub output: T,
    /// The model `OpenRouter` reports wrote it, when the reply names one.
    pub model: Option<String>,
    /// `OpenRouter`'s generation ID, when the reply carries one.
    pub generation_id: Option<String>,
    /// The tokens and cost the reply reported.
    pub usage: Usage,
}

/// The reply fields read from the provider's reply document, which Rig keeps
/// on each call for typed inspection: a run's calls don't carry the model.
#[derive(Deserialize)]
struct Reported {
    model: Option<String>,
}

impl OpenRouter {
    /// Runs one generator agent on `request`, within the generator time
    /// limit, and decodes its structured output as `T`, whose JSON schema
    /// the request carries. It ends early as [`AiError::Cancelled`] when
    /// `cancel` is cancelled.
    ///
    /// Output that doesn't decode as `T` is [`AiError::InvalidReply`], never
    /// coerced (rule 37).
    #[tracing::instrument(name = "generator", skip_all, fields(model = %request.model))]
    pub async fn generate<T>(
        &self,
        request: &GeneratorRequest,
        cancel: &CancellationToken,
    ) -> Result<GeneratorReply<T>, AiError>
    where
        T: JsonSchema + DeserializeOwned + Send + 'static,
    {
        let model = request.model.as_str();
        let provider = OpenAIConfig::with_key(&OPENROUTER, self.key_for_auth_header())
            .with_base_url(self.base_url())
            .connect(self.generator_http());
        let agent = AgentBuilder::new(provider.chat(model))
            .preamble(&request.preamble)
            .provider_option(OpenRouterOptions::new().provider(zero_retention()))
            .record_content_telemetry(false)
            // One paid call: nothing continues or replays it (rule 23).
            .default_max_turns(1)
            .build();
        let response = bounded(self.timeouts().generator, cancel, async {
            agent
                .prompt_typed::<T>(request.input.as_str())
                .await
                .map_err(|error| failure(&error, model))
        })
        .await?;
        let call = response.completion_calls.last();
        Ok(GeneratorReply {
            output: response.output,
            model: call
                .and_then(|call| Reported::deserialize(&call.raw).ok())
                .and_then(|reported| reported.model),
            generation_id: call.and_then(|call| call.response_id.clone()),
            usage: Usage::from_rig(&response.usage),
        })
    }
}

/// Only endpoints that keep no prompts, and no provider that collects data.
fn zero_retention() -> ProviderPreferences {
    ProviderPreferences::new()
        .zdr(true)
        .data_collection(DataCollection::Deny)
}

fn failure(error: &StructuredOutputError, model: &str) -> AiError {
    match error {
        StructuredOutputError::Prompt(PromptError::Provider(error)) => {
            AiError::from_provider(error, model, Routing::ZeroRetention)
        }
        StructuredOutputError::Prompt(PromptError::Report(report)) => {
            AiError::from_report(report, model, Routing::ZeroRetention)
        }
        StructuredOutputError::Prompt(PromptError::Cancelled { .. }) => AiError::Cancelled,
        // Output that doesn't decode, no output, or a reply that asked for
        // another turn.
        _ => AiError::InvalidReply,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;
    use crate::Timeouts;
    use crate::replay::{SampleQuestion, connection, received, sample_generator_request, server};

    const OUTPUT: &str = include_str!("../fixtures/generator-output.json");

    async fn generate<T>(reply: &str) -> (Result<GeneratorReply<T>, AiError>, Value)
    where
        T: JsonSchema + DeserializeOwned + Send + 'static,
    {
        let server = server("/chat/completions", 200, reply, Duration::ZERO).await;
        let result = connection(&server, Timeouts::default())
            .generate::<T>(&sample_generator_request(), &CancellationToken::new())
            .await;
        let (auth, body) = received(&server).await;
        assert_eq!(auth, "Bearer sk-or-v1-test");
        (result, body)
    }

    #[tokio::test]
    async fn a_generator_request_requires_zero_retention_and_denies_data_collection() {
        let (_, body) = generate::<SampleQuestion>(OUTPUT).await;

        assert_eq!(
            body,
            json!({
                "model": "anthropic/claude-sonnet-5.5",
                "messages": [
                    {
                        "role": "system",
                        "content": [{
                            "type": "text",
                            "text": "You write one multiple-choice question about the change you are given, with three options and the letter of the correct one.",
                        }],
                    },
                    {
                        "role": "user",
                        "content": "{\"change\":{\"summary\":\"Retry a card charge after a timeout.\",\"diff\":\"-    let key = OperationKey::new();\\n+    let key = original.key.clone();\\n     client.charge(amount, key).await?;\"}}",
                    },
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "SampleQuestion",
                        "strict": true,
                        "schema": {
                            "$schema": "https://json-schema.org/draft/2020-12/schema",
                            "title": "SampleQuestion",
                            "description": "The structured output the sample generator request asks for.",
                            "type": "object",
                            "properties": {
                                "stem": { "type": "string", "description": "The question." },
                                "options": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Three options, A to C.",
                                },
                                "answer_key": {
                                    "type": "string",
                                    "description": "The correct option's letter.",
                                },
                            },
                            "required": ["stem", "options", "answer_key"],
                            "additionalProperties": false,
                        },
                    },
                },
                "provider": { "zdr": true, "data_collection": "deny" },
            })
        );
    }

    #[tokio::test]
    async fn structured_output_decodes_with_the_model_generation_id_and_usage() {
        let (reply, _) = generate::<SampleQuestion>(OUTPUT).await;

        assert_eq!(
            reply,
            Ok(GeneratorReply {
                output: SampleQuestion {
                    stem: "A change to a payment retry path replaces `let key = OperationKey::new();` \
                           with `let key = original.key.clone();` before calling \
                           `client.charge(amount, key).await?`. After a timeout, why is reusing the \
                           original operation key the correct behavior?"
                        .to_owned(),
                    options: vec![
                        "It lets the payment provider recognize the retry as the same operation, so \
                         the card is not charged twice if the first attempt actually succeeded."
                            .to_owned(),
                        "It makes the retry faster because the provider skips validating the card \
                         when the key has been seen before."
                            .to_owned(),
                        "It guarantees that the retry will succeed, because the provider only \
                         rejects requests that carry a brand-new key."
                            .to_owned(),
                    ],
                    answer_key: "A".to_owned(),
                },
                model: Some("anthropic/claude-sonnet-5.5".to_owned()),
                generation_id: Some("gen-1792535217-56Y3dV6KOlgXlVIuOt0d".to_owned()),
                usage: Usage {
                    input_tokens: Some(466),
                    output_tokens: Some(207),
                    cost: Some(0.003_002),
                },
            })
        );
    }

    /// A type the recorded output doesn't fit: it has no `rubric`.
    #[derive(Debug, Deserialize, JsonSchema)]
    struct WithRubric {
        #[expect(dead_code, reason = "the output never decodes, so it is never read")]
        rubric: Vec<String>,
    }

    #[tokio::test]
    async fn output_that_does_not_decode_into_the_type_is_an_invalid_reply() {
        let (reply, _) = generate::<WithRubric>(OUTPUT).await;

        assert_eq!(reply.err(), Some(AiError::InvalidReply));
    }
}
