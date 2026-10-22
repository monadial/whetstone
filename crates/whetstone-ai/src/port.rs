//! The AI port: what Practice asks of Jev and the generator, in Whetstone
//! types only.
//!
//! Each call takes what it needs by reference and returns a request on its
//! way: a future that owns everything it holds, so a background job can run
//! it on the runtime ([`Pending`]). It resolves to Jev's answers or the
//! generator's writing, as returned, with the provenance of what produced
//! them, or to the [`RequestFailure`] that stopped it. Nothing is retried
//! automatically: Retry is always the learner's.
//!
//! Each call is stamped by its caller with Whetstone's own identity for the
//! request and the versions behind it ([`Stamp`]), which the reply's
//! provenance carries.
//!
//! A reasoning evaluation, the explanation of what the reasoning missed
//! and its check are asked only with a session store's acknowledgement that
//! the response is saved ([`Saved`]), so none is ever paid for while the
//! response could still be lost (rule 39).

use std::fmt;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;

use whetstone_core::{
    Area, AreaId, Candidate, CriterionOutcome, Evidence, Probability, Provenance, Question,
    QuestionCount, RequestId, Versions, WrittenCandidate,
};

use crate::explanation::{ParagraphCheck, WrittenParagraph};
use crate::question_check::{CheckCallAnswers, QuestionCheckCall};
use crate::reasoning::QuestionWithoutKey;

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
        stamp: Stamp,
        evidence: &Evidence,
        areas: &[Area],
    ) -> Pending<Vec<(AreaId, Probability)>>;

    /// The generator writes `count` candidate questions about `area` from
    /// `evidence`, as it wrote them, before any check, except that each
    /// one's options are shuffled ([`shuffle_options`]), so where the answer
    /// key sits carries no signal. A scripted AI returns its candidates as
    /// scripted.
    ///
    /// [`shuffle_options`]: whetstone_core::shuffle_options
    fn write_candidates(
        &self,
        stamp: Stamp,
        evidence: &Evidence,
        area: &Area,
        count: CandidateCount,
    ) -> Pending<Vec<WrittenCandidate>>;

    /// One call of Jev's question check of `candidate` against `evidence`:
    /// an answer to each of [`Candidate::checks`] that `call` asks, a yes/no
    /// answer or, for the answer probe, a probability for each option, and,
    /// for the candidate call, the depth, as returned. A question check is
    /// four calls, sent together, each sent only what its questions are
    /// about ([`QuestionCheckCall`]), so the option ratings never see the
    /// answer key, and the answer probe sees only the stem and the options'
    /// text. [`CheckCallAnswers::merged`] makes the four calls' answers one
    /// [`QuestionCheckAnswers`].
    ///
    /// [`QuestionCheckAnswers`]: whetstone_core::QuestionCheckAnswers
    fn check_question(
        &self,
        stamp: Stamp,
        call: QuestionCheckCall,
        evidence: &Evidence,
        candidate: &Candidate,
    ) -> Pending<CheckCallAnswers>;

    /// Jev's assessment of the reasoning of the response the session store
    /// acknowledged saving with `saved`, written for `question`, against
    /// `evidence`: for each criterion of the rubric, in rubric order, the
    /// probability that the reasoning meets it, as returned. Jev is never
    /// told the chosen option or the answer key, which neither `saved` nor
    /// `question` gives.
    fn assess_reasoning(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: QuestionWithoutKey<'_>,
    ) -> Pending<Vec<Probability>>;

    /// The generator explains what the reasoning of the response the
    /// session store acknowledged saving with `saved`, written for
    /// `question`, missed, against `evidence`: one paragraph, with
    /// citations, for each of `criteria`, each a criterion's place in the
    /// rubric, counting from 0, and its outcome, not met or needs review. For
    /// one that needs review, the paragraph describes what meeting it would
    /// look like, without claiming the learner missed it. The paragraphs
    /// come in rubric order, as written, their citations unchecked.
    ///
    /// The outcomes are Jev's, and stand: the explanation never changes
    /// them. With no criteria, or one outside the rubric, nothing is sent,
    /// and the request is invalid.
    fn explain(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: &Question,
        criteria: &[(usize, CriterionOutcome)],
    ) -> Pending<Vec<WrittenParagraph>>;

    /// Jev's explanation check of `paragraphs`, explaining criteria of
    /// `question` for the response the session store acknowledged saving
    /// with `saved`, against `evidence`: for each paragraph, in order,
    /// whether it is supported by the evidence and whether it agrees with
    /// its criterion's outcome, as returned. It sends no reasoning, but is
    /// asked only with the acknowledgement, as every paid request about a
    /// response is (rule 39).
    fn check_explanation(
        &self,
        stamp: Stamp,
        saved: &dyn Saved,
        evidence: &Evidence,
        question: &Question,
        paragraphs: &[WrittenParagraph],
    ) -> Pending<Vec<ParagraphCheck>>;
}

/// A session store's acknowledgement that it saved a response whose
/// reasoning is written, which gives that reasoning as saved. A reasoning
/// evaluation is built from one, so it is never paid for while the response
/// could still be lost, or for a response with no reasoning written (rule
/// 39), and it sends exactly what was saved. The app's `SavedReasoning`,
/// which only a `SavedResponse` from its session store makes, and only when
/// the reasoning is written, is the only one; nothing else implements this
/// outside tests.
pub trait Saved: fmt::Debug + Send + Sync {
    /// The learner's reasoning, as the store saved it.
    fn reasoning(&self) -> &str;
}

/// What a caller stamps on each request it makes: Whetstone's own identity
/// for the request, and the versions behind it. The reply's provenance
/// carries both.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    /// Whetstone's own identity for the request, unique within a run.
    pub request: RequestId,
    /// Whetstone's version, the base prompts', the assessment policy's, and
    /// the file of each area the request is about.
    pub versions: Versions,
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
    /// What produced it: the request's stamp, the models asked and
    /// answering, for Jev the questions sent with their answers, and the
    /// usage and cost where the reply reports them. It is boxed, as it
    /// holds the versions and the questions, to keep the app's messages
    /// small.
    pub provenance: Box<Provenance>,
}

/// Why a request to Jev or the generator failed. Each is named to the
/// learner, with Retry, and none is ever shown as a judgement: its
/// `Display` is its name, as in "Writing questions failed: timed out.".
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
    /// `OpenRouter` couldn't be reached, or the model's provider failed.
    #[error("OpenRouter or the model's provider is unavailable")]
    ProviderUnavailable,
    /// The model has no endpoint that keeps no prompts, so `OpenRouter`
    /// refused the route. Only a generator request asks for one until #69.
    #[error("no zero-retention endpoint for the model (refused by OpenRouter)")]
    NoZeroRetentionEndpoint,
    /// No reply came in time.
    #[error("timed out")]
    TimedOut,
    /// A reply came that could not be read: an answer missing, unexpected
    /// or out of range. It is never coerced (rule 37).
    #[error("the reply couldn't be read")]
    InvalidReply,
    /// The request was refused as invalid before any reply: `OpenRouter`
    /// doesn't know the model, say, or its moderation blocked the input. It
    /// is also what a request that could not be built at all is.
    #[error("OpenRouter refused the request as invalid")]
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
