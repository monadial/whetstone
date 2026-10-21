//! Every request a Practice sends to Jev and the generator, as each ends:
//! the provenance of its reply, or, for a request that failed or that the
//! learner cancelled, one that reports no usage or cost. The session record
//! starts with what it holds, and the summary adds up the session's cost
//! from it, for each role and in all, counting each request that reported
//! no cost instead of estimating it.

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

    /// What the requests cost so far, as their replies reported it, while
    /// `running` more still run.
    pub(crate) fn spending(&self, running: usize) -> Spending {
        let of = |role| {
            SessionCost::of(
                self.provenance
                    .iter()
                    .filter(|provenance| provenance.role() == role),
            )
        };
        Spending {
            jev: of(Role::Jev),
            generator: of(Role::Generator),
            total: SessionCost::of(&self.provenance),
            running,
        }
    }
}

/// What the requests a Practice sent cost so far, as their replies reported
/// it, each cost with how many of its requests reported none, and how many
/// requests still run, whose cost isn't known until they reply.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spending {
    /// Jev's requests' cost.
    pub(crate) jev: SessionCost,
    /// The generator's requests' cost.
    pub(crate) generator: SessionCost,
    /// Every request's cost: Jev's and the generator's together.
    pub(crate) total: SessionCost,
    /// How many requests still run.
    pub(crate) running: usize,
}
