//! The AI port: what Practice asks of Jev and the generator, in Whetstone
//! types only.
//!
//! Each call takes what it needs by reference and returns a request on its
//! way: a future that owns everything it holds, so a background job can run
//! it on the runtime ([`Pending`]). It resolves to Jev's answers or the
//! generator's writing, as returned, with the provenance of what produced
//! them, or to the [`RequestFailure`] that stopped it. Nothing is retried
//! automatically: Retry is always the learner's.

use std::fmt;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;

use whetstone_core::{
    Area, AreaId, Candidate, Evidence, Probability, Provenance, QuestionCheckAnswers,
    QuestionCount, WrittenCandidate,
};

/// What Practice asks of Jev and the generator. The real implementation
/// calls `OpenRouter`; a scripted one stands in for it in tests.
pub trait Ai: fmt::Debug + Send + Sync {
    /// Jev's relevance gate, in one call, before any question is written:
    /// for each of `areas`, the chosen areas, the probability that the
    /// change in `evidence` is worth asking about in it, in their order, as
    /// returned. Jev judges each area by its name and description; area
    /// prompts never reach it.
    fn check_areas(
        &self,
        evidence: &Evidence,
        areas: &[Area],
    ) -> Pending<Vec<(AreaId, Probability)>>;

    /// The generator writes `count` candidate questions about `area` from
    /// `evidence`, as it wrote them, before any check.
    fn write_candidates(
        &self,
        evidence: &Evidence,
        area: &Area,
        count: CandidateCount,
    ) -> Pending<Vec<WrittenCandidate>>;

    /// Jev's question check of `candidate` against `evidence`, in one call:
    /// a yes/no answer to each of [`Candidate::checks`], as returned.
    fn check_question(
        &self,
        evidence: &Evidence,
        candidate: &Candidate,
    ) -> Pending<QuestionCheckAnswers>;
}

/// A request on its way to Jev or the generator: it resolves to the reply
/// or to why the request failed. It owns everything it holds, so it can run
/// as a background job, and dropping it stops the request.
pub type Pending<T> = Pin<Box<dyn Future<Output = Result<Reply<T>, RequestFailure>> + Send>>;

/// What a request returned, with what produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply<T> {
    /// The answers or writing, as returned.
    pub content: T,
    /// The models asked and answering, and the usage and cost where the
    /// reply reports them.
    pub provenance: Provenance,
}

/// Why a request to Jev or the generator failed. Each is named to the
/// learner, with Retry, and none is ever shown as a judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RequestFailure {
    /// No API key was found.
    #[error("no OpenRouter key was found")]
    NoKey,
    /// The provider refused the API key.
    #[error("OpenRouter refused the key")]
    KeyRefused,
    /// The account has no credits left.
    #[error("out of OpenRouter credits")]
    OutOfCredits,
    /// Too many requests too quickly.
    #[error("rate limited")]
    RateLimited,
    /// The provider or the model is unavailable.
    #[error("the provider is unavailable")]
    ProviderUnavailable,
    /// The model has no endpoint that keeps no prompts.
    #[error("the model has no zero-retention endpoint")]
    NoZeroRetentionEndpoint,
    /// No reply came in time.
    #[error("timed out")]
    TimedOut,
    /// A reply came that could not be read: an answer missing, unexpected
    /// or out of range. It is never coerced (rule 37).
    #[error("the reply could not be read")]
    InvalidReply,
    /// The request was refused as invalid before any reply: `OpenRouter`
    /// doesn't know the model, say, or its moderation blocked the input. It
    /// is also what a request that could not be built at all is.
    #[error("the request was invalid")]
    InvalidRequest,
    /// The request was cancelled before its reply came.
    #[error("cancelled")]
    Cancelled,
}

/// How many candidates the generator writes for one area: two more than the
/// questions asked for, split across the chosen areas and rounded up, and at
/// least one, so that picking can still fill the set when some fail their
/// checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateCount(NonZeroUsize);

impl CandidateCount {
    /// The count for each of `areas` chosen areas when `questions` are asked
    /// for: ⌈(`questions` + 2) ÷ `areas`⌉, and at least one.
    #[must_use]
    pub fn per_area(questions: QuestionCount, areas: usize) -> Self {
        let wanted = NonZeroUsize::MIN.saturating_add(usize::from(questions.get()) + 1);
        Self(wanted.div_ceil(NonZeroUsize::new(areas).unwrap_or(NonZeroUsize::MIN)))
    }

    /// The count as a number.
    #[must_use]
    pub fn get(self) -> usize {
        self.0.get()
    }
}

impl From<CandidateCount> for NonZeroUsize {
    fn from(count: CandidateCount) -> Self {
        count.0
    }
}

#[cfg(test)]
mod tests {
    use whetstone_core::QuestionCount;

    use super::*;

    fn questions(count: i64) -> QuestionCount {
        QuestionCount::try_from(count).expect("from 3 to 5")
    }

    #[test]
    fn each_area_is_asked_for_two_more_than_the_questions_split_across_the_areas_rounded_up() {
        for (count, areas, each) in [
            (4, 1, 6),
            (4, 2, 3),
            (4, 4, 2),
            (4, 5, 2),
            (5, 3, 3),
            (3, 2, 3),
            (3, 5, 1),
        ] {
            assert_eq!(
                CandidateCount::per_area(questions(count), areas).get(),
                each,
                "{count} questions over {areas} areas"
            );
        }
    }

    #[test]
    fn each_area_is_asked_for_at_least_one() {
        assert_eq!(CandidateCount::per_area(questions(3), 12).get(), 1);
    }
}
