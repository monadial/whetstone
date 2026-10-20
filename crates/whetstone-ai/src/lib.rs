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
//! Nothing here logs a key, a prompt, evidence, questions or output (rules 43
//! and 44): spans carry the model alone, and Rig's content telemetry stays off.

mod connection;
mod failure;
mod generator;
mod jev;
#[cfg(test)]
mod record;
#[cfg(test)]
mod replay;
mod usage;

pub use crate::connection::{
    CONNECT_TIMEOUT, GENERATOR_TIMEOUT, JEV_TIMEOUT, OPENROUTER_BASE_URL, OpenRouter, Timeouts,
};
pub use crate::failure::AiError;
pub use crate::generator::{GeneratorReply, GeneratorRequest};
pub use crate::jev::{JevAnswer, JevQuestion, JevReply, JevRequest, YesNoMeanings};
pub use crate::usage::Usage;
/// The key's type, re-exported so that callers need no `secrecy` dependency.
pub use secrecy::SecretString;
