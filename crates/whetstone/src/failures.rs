//! A failed request to Jev or the generator, as the learner reads it: the
//! failure's name, which the AI port gives, then what they can do about it.
//!
//! A model `OpenRouter` refuses for having no zero-retention endpoint is
//! remembered for the rest of the run, and Settings marks it. Nothing is
//! written to the config: the learner changes the model there themselves.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use whetstone_ai::{Models, RequestFailure};
use whetstone_core::{ModelId, Role};

use crate::config::OPENROUTER_KEY_VARIABLE;

/// What Settings shows against a model `OpenRouter` refused for having no
/// zero-retention endpoint.
pub(crate) const REFUSED_MARK: &str = "no zero-retention endpoint (refused by OpenRouter)";

/// The models `OpenRouter` refused during this run because none of their
/// endpoints keeps no prompts, which Settings marks until Whetstone quits.
/// The app keeps one for the whole run, shared with the screens that ask
/// Jev and the generator.
#[derive(Debug, Default)]
pub(crate) struct RefusedModels(Mutex<BTreeSet<ModelId>>);

impl RefusedModels {
    /// Remembers that `OpenRouter` refused `model`.
    fn refuse(&self, model: &ModelId) {
        self.models().insert(model.clone());
    }

    /// Whether `OpenRouter` refused `model` during this run.
    pub(crate) fn refused(&self, model: &ModelId) -> bool {
        self.models().contains(model)
    }

    fn models(&self) -> MutexGuard<'_, BTreeSet<ModelId>> {
        // Only the UI loop's thread locks it, and never across a wait, so a
        // poisoned lock can only follow a panic that is already ending the
        // app; the set is still whole.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The failed requests of a screen that asks Jev and the generator: what
/// the learner can do about each, and the models `OpenRouter` refused, which
/// the run remembers.
#[derive(Debug, Clone)]
pub(crate) struct FailedRequests {
    /// The models the screen asks, as the config names them.
    models: Models,
    refused: Arc<RefusedModels>,
}

impl FailedRequests {
    /// Failed requests to `models`, remembering refused ones in `refused`.
    pub(crate) fn new(models: Models, refused: &Arc<RefusedModels>) -> Self {
        Self {
            models,
            refused: Arc::clone(refused),
        }
    }

    /// The model a request to `role` asks.
    fn model(&self, role: Role) -> &ModelId {
        match role {
            Role::Jev => &self.models.jev,
            Role::Generator => &self.models.generator,
        }
    }

    /// Remembers the model a request to `role` asked when `failure` is
    /// `OpenRouter` refusing it for having no zero-retention endpoint, so that
    /// Settings marks it for the rest of the run. Until Jev's requests ask
    /// for zero retention (#69), only the generator's model can be.
    pub(crate) fn note_refusal(&self, role: Role, failure: RequestFailure) {
        if failure == RequestFailure::NoZeroRetentionEndpoint {
            let model = self.model(role);
            tracing::info!(%model, "OpenRouter refused a model: no zero-retention endpoint");
            self.refused.refuse(model);
        }
    }

    /// What the learner can do about `failure` of a request to `role`, in
    /// plain words, after the failure's name.
    pub(crate) fn what_to_do(&self, role: Role, failure: RequestFailure) -> String {
        let model = self.model(role);
        let (who, section) = match role {
            Role::Jev => ("Jev", "jev"),
            Role::Generator => ("The generator", "generator"),
        };
        match failure {
            RequestFailure::NoKey => {
                format!("Set {OPENROUTER_KEY_VARIABLE} in your shell, then start Whetstone again.")
            }
            RequestFailure::KeyRefused => format!(
                "Check the key in {OPENROUTER_KEY_VARIABLE}, which Whetstone reads as it starts."
            ),
            RequestFailure::OutOfCredits => {
                "Add credits to your OpenRouter account, then Retry.".to_owned()
            }
            RequestFailure::RateLimited => "OpenRouter had too many requests from your key too \
                quickly. Wait a moment, then Retry."
                .to_owned(),
            RequestFailure::ProviderUnavailable => format!(
                "OpenRouter couldn't be reached, or {model}'s provider failed. Check your \
                 connection, then Retry."
            ),
            RequestFailure::NoZeroRetentionEndpoint => format!(
                "OpenRouter has no endpoint for {model} that keeps no prompts, so nothing went \
                 to the model. Choose another model under [{section}] in config.toml; Settings \
                 marks this one."
            ),
            RequestFailure::TimedOut => {
                format!("{who} didn't reply in time. Retry, or try again later.")
            }
            RequestFailure::InvalidReply => match role {
                Role::Jev => "Nothing in Jev's reply is used, and none of it is shown as a \
                    judgement."
                    .to_owned(),
                Role::Generator => "Nothing in the generator's reply is used.".to_owned(),
            },
            RequestFailure::InvalidRequest => format!(
                "OpenRouter may not know {model}, or its moderation blocked the request. Check \
                 the model under [{section}] in config.toml."
            ),
            RequestFailure::Cancelled => {
                "The request was cancelled before its reply came.".to_owned()
            }
        }
    }
}
