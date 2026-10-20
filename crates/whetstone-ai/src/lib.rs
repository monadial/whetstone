//! Whetstone's calls to Jev and the generator, through `OpenRouter` on Rig.
//!
//! This is the only crate that depends on Rig (ADR 0001, rule 36). It is the
//! layer under the AI port: [`OpenRouter`] holds the key, the base URL and
//! the time limits, and makes one call at a time:
//!
//! - [`OpenRouter::evaluate`] sends one [`JevRequest`] to Jev on `OpenRouter`'s
//!   `/systemone` and returns Jev's answers as returned;
//! - [`OpenRouter::generate`] runs one generator agent, with no tools, and
//!   decodes its structured output into the caller's type.
//!
//! Both return the reported model and [`Usage`], and fail with a named
//! [`AiError`]. Nothing is retried (rule 23), and no Rig type crosses this
//! interface.
//!
//! Above that layer, each AI call has a builder and a decoder, between
//! whetstone-core's domain and these requests:
//!
//! - [`gate_request`] and [`gate_answers`] ask the relevance gate;
//! - [`candidates_request`] and [`CandidatesOutput`] write candidates;
//! - [`question_check_request`] and [`question_check_answers`] check one;
//! - [`reasoning_request`] and [`reasoning_answers`] assess a response's
//!   reasoning;
//! - [`explanation_request`], [`explanation_paragraphs`],
//!   [`explanation_check_request`] and [`explanation_check_answers`] explain
//!   what it missed, and check the explanation.
//!
//! [`jev_exchanges`] keeps, for provenance, every question a Jev call sent
//! with its answer. The instructions are Whetstone's base prompts, bundled
//! into this crate, whose version is [`base_prompts_version`], or
//! [`base_prompts_hash`] for provenance. Evidence goes out only as the
//! learner approved it, and area prompts only to the generator.
//!
//! Nothing here logs a key, a prompt, evidence, questions or output (rules 43
//! and 44): spans carry the model alone, and Rig's content telemetry stays off.

mod answers;
mod candidates;
mod connection;
mod evidence;
mod exchanges;
mod explanation;
mod failure;
mod gate;
mod generator;
mod jev;
mod output;
mod prompts;
mod question_check;
mod reasoning;
#[cfg(test)]
mod record;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod samples;
mod usage;

pub use crate::candidates::{CandidatesOutput, candidates_request};
pub use crate::connection::{
    CONNECT_TIMEOUT, GENERATOR_TIMEOUT, JEV_TIMEOUT, OPENROUTER_BASE_URL, OpenRouter, Timeouts,
};
pub use crate::exchanges::jev_exchanges;
pub use crate::explanation::{
    ExplanationOutput, ParagraphCheck, WrittenParagraph, explanation_check_answers,
    explanation_check_request, explanation_paragraphs, explanation_request,
};
pub use crate::failure::AiError;
pub use crate::gate::{gate_answers, gate_request};
pub use crate::generator::{GeneratorReply, GeneratorRequest};
pub use crate::jev::{JevAnswer, JevQuestion, JevReply, JevRequest, YesNoMeanings};
pub use crate::prompts::{base_prompts_hash, base_prompts_version};
pub use crate::question_check::{question_check_answers, question_check_request};
pub use crate::reasoning::{QuestionWithoutKey, reasoning_answers, reasoning_request};
pub use crate::usage::Usage;
/// The key's type, re-exported so that callers need no `secrecy` dependency.
pub use secrecy::SecretString;
