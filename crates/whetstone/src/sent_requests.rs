//! Every request a Practice sends to Jev and the generator, as each ends:
//! the provenance of its reply, or, for a request that failed or that the
//! learner cancelled, one that reports no usage or cost. The session record
//! starts with what it holds, and the summary adds up the session's cost
//! from it, counting each request that reported no cost instead of
//! estimating it.

use whetstone_ai::{Models, Reply, RequestFailure};
use whetstone_core::{Provenance, Role, SessionCost};

/// The requests a Practice sent, each with its provenance, in the order
/// they ended.
#[derive(Debug, Clone)]
pub(crate) struct SentRequests {
    /// The models the requests ask, as the config names them, for the
    /// provenance of a request with no reply.
    models: Models,
    provenance: Vec<Provenance>,
}

impl SentRequests {
    /// No request yet, to `models`.
    pub(crate) fn new(models: Models) -> Self {
        Self {
            models,
            provenance: Vec::new(),
        }
    }

    /// Keeps what produced `reply`, to a request to `role`: its provenance,
    /// or, when it failed, provenance that reports no usage or cost. Returns
    /// its content, or why it failed.
    pub(crate) fn ended<T>(
        &mut self,
        role: Role,
        reply: Result<Reply<T>, RequestFailure>,
    ) -> Result<T, RequestFailure> {
        match reply {
            Ok(reply) => {
                self.provenance.push(reply.provenance);
                Ok(reply.content)
            }
            Err(failure) => {
                self.unanswered(role);
                Err(failure)
            }
        }
    }

    /// Keeps a request to `role` that ended with no reply, as one does when
    /// it fails or the learner cancels it: it reports no usage or cost.
    pub(crate) fn unanswered(&mut self, role: Role) {
        let model = match role {
            Role::Jev => &self.models.jev,
            Role::Generator => &self.models.generator,
        };
        self.provenance.push(Provenance::new(role, model.clone()));
    }

    /// Each request's provenance, in the order they ended.
    pub(crate) fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }

    /// What the requests cost, as their replies reported it.
    pub(crate) fn cost(&self) -> SessionCost {
        SessionCost::of(&self.provenance)
    }
}
