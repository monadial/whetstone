//! Every request a Practice sends to Jev and the generator, from the stamp
//! it goes out with to how it ends: what it asked, the provenance of its
//! reply, or, for a request that failed or that the learner cancelled,
//! provenance that reports no usage or cost, and how it ended. The session
//! record starts with what it holds, and each request that ends after is
//! saved to the record as it ends. The summary adds up the session's cost
//! from it, for each role and in all, counting each request that reported
//! no cost instead of estimating it.
//!
//! Each request is stamped as it is sent with Whetstone's own identity for
//! it, counting from 1 in the order sent, and the versions behind it:
//! Whetstone's, the base prompts', the assessment policy's, and each area
//! file's that went into it.

use whetstone_ai::{Models, Reply, RequestFailure, Stamp, base_prompts_hash};
use whetstone_core::{
    AreaFileVersion, AreaId, AssessmentPolicy, CandidateId, ContentHash, Provenance, RequestId,
    Role, SessionCost, Versions,
};

use crate::session_record::SavedResponse;

/// What a request asked, which its result must carry (rule 24), and which
/// the session record keeps it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asking {
    /// Jev, to decide which chosen areas are worth asking about.
    Gate,
    /// The generator, to write candidates about this area.
    Write(AreaId),
    /// Jev, to check this candidate.
    Check(CandidateId),
    /// Jev, to assess the reasoning of the response this acknowledges: its
    /// session, question and attempt.
    Evaluate(SavedResponse),
    /// The generator, to explain what the reasoning of the response this
    /// acknowledges missed.
    Explain(SavedResponse),
    /// Jev, to check the explanation of the reasoning of the response this
    /// acknowledges.
    CheckExplanation(SavedResponse),
}

/// A request a Practice sent, as it ended: what it asked, what produced its
/// result, and whether a reply came.
#[derive(Debug, Clone, PartialEq)]
pub struct SentRequest {
    /// What it asked.
    pub asking: Asking,
    /// Its identity, the models asked and answering, the versions behind
    /// it, for Jev the questions sent with their answers, and its usage and
    /// cost where the reply reported them.
    pub provenance: Provenance,
    /// Whether its reply came.
    pub ended: RequestEnded,
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestEnded {
    /// Its reply came.
    Replied,
    /// It failed, or the learner cancelled it, before a reply came.
    Failed(RequestFailure),
}

impl Asking {
    /// Who the request goes to.
    fn role(&self) -> Role {
        match self {
            Self::Gate | Self::Check(_) | Self::Evaluate(_) | Self::CheckExplanation(_) => {
                Role::Jev
            }
            Self::Write(_) | Self::Explain(_) => Role::Generator,
        }
    }

    /// The one area the request is about, or none for the relevance gate,
    /// which is about every chosen area.
    fn area(&self) -> Option<&AreaId> {
        match self {
            Self::Gate => None,
            Self::Write(area) => Some(area),
            Self::Check(candidate) => Some(candidate.area()),
            Self::Evaluate(saved) | Self::Explain(saved) | Self::CheckExplanation(saved) => {
                Some(saved.question().area())
            }
        }
    }
}

/// The requests a Practice sent, in the order they ended, and those still
/// on their way, each with its stamp.
#[derive(Debug, Clone)]
pub(crate) struct SentRequests {
    /// The models the requests ask, as the config names them, for the
    /// provenance of a request with no reply.
    models: Models,
    /// The base prompts' version, which every request goes out with.
    base_prompts: ContentHash,
    /// The chosen areas' files.
    area_files: Vec<AreaFileVersion>,
    /// The identity of the last request stamped: none is 0.
    last: u64,
    /// Each request on its way, with what it asked and its stamp.
    sending: Vec<(Asking, Stamp)>,
    ended: Vec<SentRequest>,
    /// How many of `ended` the session record holds: those before it.
    recorded: usize,
}

impl SentRequests {
    /// No request yet, to `models`, about the chosen areas, whose files
    /// are `area_files`.
    pub(crate) fn new(models: Models, area_files: Vec<AreaFileVersion>) -> Self {
        Self {
            models,
            base_prompts: base_prompts_hash(),
            area_files,
            last: 0,
            sending: Vec::new(),
            ended: Vec::new(),
            recorded: 0,
        }
    }

    /// The stamp for the next request, which asks `asking`: the next
    /// identity, and the versions behind it, with the files of the areas it
    /// is about. It is kept until the request ends.
    pub(crate) fn stamp(&mut self, asking: &Asking) -> Stamp {
        let stamp = self.next_stamp(asking);
        self.sending.push((asking.clone(), stamp.clone()));
        stamp
    }

    /// The next identity, with the versions behind a request that asks
    /// `asking`.
    fn next_stamp(&mut self, asking: &Asking) -> Stamp {
        self.last += 1;
        let area_files = self
            .area_files
            .iter()
            .filter(|file| asking.area().is_none_or(|area| file.area() == area))
            .cloned()
            .collect();
        Stamp {
            request: RequestId::new(self.last),
            versions: Versions::new(
                env!("CARGO_PKG_VERSION").to_owned(),
                self.base_prompts.clone(),
                AssessmentPolicy::current().version(),
                area_files,
            ),
        }
    }

    /// Keeps the request that asked `asking` as `reply` ended it: with its
    /// reply's provenance, or, when it failed, provenance that reports no
    /// usage or cost. Returns its content, or why it failed.
    pub(crate) fn ended<T>(
        &mut self,
        asking: &Asking,
        reply: Result<Reply<T>, RequestFailure>,
    ) -> Result<T, RequestFailure> {
        let stamp = self.take_stamp(asking);
        match reply {
            Ok(reply) => {
                self.ended.push(SentRequest {
                    asking: asking.clone(),
                    provenance: *reply.provenance,
                    ended: RequestEnded::Replied,
                });
                Ok(reply.content)
            }
            Err(failure) => {
                self.unanswered(asking, stamp, failure);
                Err(failure)
            }
        }
    }

    /// Keeps the request that asked `asking`, which the learner cancelled
    /// before its reply came: it reports no usage or cost.
    pub(crate) fn cancelled(&mut self, asking: &Asking) {
        let stamp = self.take_stamp(asking);
        self.unanswered(asking, stamp, RequestFailure::Cancelled);
    }

    /// The stamp of the request on its way that asked `asking`, which no
    /// longer is. A request is stamped as it is sent, so one is always
    /// there; were it not, a new one stands in.
    fn take_stamp(&mut self, asking: &Asking) -> Stamp {
        match self.sending.iter().position(|(sent, _)| sent == asking) {
            Some(at) => self.sending.remove(at).1,
            None => self.next_stamp(asking),
        }
    }

    /// Keeps the request stamped `stamp`, which asked `asking` and ended
    /// with `failure` before any reply: it reports no usage or cost.
    fn unanswered(&mut self, asking: &Asking, stamp: Stamp, failure: RequestFailure) {
        let role = asking.role();
        let model = match role {
            Role::Jev => &self.models.jev,
            Role::Generator => &self.models.generator,
        };
        tracing::info!(
            request = %stamp.request,
            ?role,
            %failure,
            "a request ended without a reply"
        );
        self.ended.push(SentRequest {
            asking: asking.clone(),
            provenance: Provenance::new(stamp.request, role, model.clone(), stamp.versions),
            ended: RequestEnded::Failed(failure),
        });
    }

    /// Every request that ended, in the order they ended.
    pub(crate) fn requests(&self) -> &[SentRequest] {
        &self.ended
    }

    /// The requests that ended since the session record last took them, in
    /// the order they ended, for the record to save.
    pub(crate) fn take_unrecorded(&mut self) -> Vec<SentRequest> {
        let unrecorded = self.ended[self.recorded..].to_vec();
        self.recorded = self.ended.len();
        unrecorded
    }

    /// What the requests cost so far, as their replies reported it, while
    /// `running` more still run.
    pub(crate) fn spending(&self, running: usize) -> Spending {
        let provenance = || self.ended.iter().map(|sent| &sent.provenance);
        let of = |role| SessionCost::of(provenance().filter(|sent| sent.role() == role));
        Spending {
            jev: of(Role::Jev),
            generator: of(Role::Generator),
            total: SessionCost::of(provenance()),
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
