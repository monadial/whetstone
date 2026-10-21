//! The assessment policy: the versioned cut-offs that turn Jev's
//! probabilities into clear yes, unclear or clear no, for each kind of check.
//!
//! The default cut-offs are built in. Changing one is a recorded decision
//! that bumps the version (`CODING_STANDARDS.md` rule 38), and
//! `docs/assessment-policy.md` records each version's reason and evidence.
//! Every released policy stays built in, so an outcome recorded under an
//! older one is still identified by its version and cut-offs. A learner may
//! override any job's cut-offs in their own config, and every outcome
//! records the cut-offs that banded it and whether they are the policy's
//! defaults or the learner's own, so it can be traced to them.

use std::fmt;

use crate::jev::Probability;

/// The cut-offs Whetstone applies to Jev's answers, with their version and
/// whether they have been checked against labelled examples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessmentPolicy {
    version: PolicyVersion,
    validation: Validation,
    /// Each job's cut-offs and whose they are, in [`Job::ALL`] order.
    cut_offs: [(CutOffs, CutOffsOrigin); Job::ALL.len()],
}

/// An assessment policy's version. Every outcome carries the version of the
/// policy that banded it.
///
/// ```
/// use whetstone_core::AssessmentPolicy;
///
/// assert_eq!(AssessmentPolicy::current().version().to_string(), "1");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyVersion(u32);

/// Whether a policy's cut-offs have been checked against labelled examples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validation {
    /// Not yet checked: the cut-offs are a starting point.
    Unvalidated,
    /// Checked against labelled examples, by a recorded decision.
    Validated,
}

/// A kind of check Jev answers, each with its own cut-offs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Job {
    /// Whether a change is worth asking about for an area.
    RelevanceGate,
    /// A candidate's checks 1–6 but the giveaway check: answerable from the
    /// evidence, about its area, needs this change, each criterion, and
    /// whether each criterion asks only what the question needs.
    QuestionCheck,
    /// A candidate's check 4: its stem and options don't give the answer
    /// away. Jev scores it on a scale of its own, so it has its own
    /// cut-offs.
    GiveawayCheck,
    /// Whether an option correctly answers its question.
    OptionRating,
    /// Whether a rationale, the "why it matters" line or a hint holds up.
    RationaleAndHintChecks,
    /// Whether the learner's reasoning meets a criterion.
    ReasoningCriterion,
    /// Whether an explanation paragraph holds up.
    ExplanationCheck,
}

/// Where a probability falls under a job's cut-offs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// At or above the upper cut-off.
    ClearYes,
    /// Between the cut-offs.
    Unclear,
    /// At or below the lower cut-off.
    ClearNo,
}

/// A job's cut-offs: a probability at or above `upper` is clear yes, and one
/// at or below `lower` is clear no. Each is a number from 0 to 1, and
/// `lower` is below `upper`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutOffs {
    upper: f64,
    lower: f64,
}

/// Never NaN: [`CutOffs::new`] refuses it, so equality is total.
impl Eq for CutOffs {}

/// Cut-offs that can't band a job's answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCutOffs {
    /// The upper cut-off isn't a number from 0 to 1.
    #[error("`upper` must be a number from 0 to 1")]
    Upper,
    /// The lower cut-off isn't a number from 0 to 1.
    #[error("`lower` must be a number from 0 to 1")]
    Lower,
    /// The lower cut-off isn't below the upper.
    #[error("`lower` must be below `upper`")]
    NotBelow,
}

/// Whose cut-offs band a job's answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CutOffsOrigin {
    /// The policy's own defaults.
    Default,
    /// The learner's own, from their config: outcomes banded with them
    /// aren't comparable with the policy's.
    Learner,
}

/// The cut-offs a job's answers were banded with: the version of the policy,
/// the job's cut-offs, and whether they are the policy's defaults or the
/// learner's own. Every outcome carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedCutOffs {
    policy: PolicyVersion,
    cut_offs: CutOffs,
    origin: CutOffsOrigin,
}

/// Every policy released, oldest first. The last is the one this release
/// applies by default.
static RELEASED: [AssessmentPolicy; 2] = [POLICY_0, POLICY_1];

/// Policy 0: unvalidated, with every job at 0.80 and 0.20.
const POLICY_0: AssessmentPolicy = AssessmentPolicy {
    version: PolicyVersion(0),
    validation: Validation::Unvalidated,
    cut_offs: [(CutOffs::POLICY_0, CutOffsOrigin::Default); Job::ALL.len()],
};

/// Policy 1: unvalidated, with the giveaway check at 0.50 and 0.20, and
/// every other job at policy 0's 0.80 and 0.20.
const POLICY_1: AssessmentPolicy = {
    let mut cut_offs = POLICY_0.cut_offs;
    cut_offs[Job::GiveawayCheck.index()] = (
        CutOffs {
            upper: 0.50,
            lower: 0.20,
        },
        CutOffsOrigin::Default,
    );
    AssessmentPolicy {
        version: PolicyVersion(1),
        validation: Validation::Unvalidated,
        cut_offs,
    }
};

impl AssessmentPolicy {
    /// The policy this release applies by default: version 1, unvalidated,
    /// with the giveaway check's upper cut-off at 0.50 and every other
    /// job's at 0.80, and every job's lower at 0.20. Practice applies
    /// [`AssessmentPolicy::with_cut_offs`] instead, with the cut-offs the
    /// learner's config sets.
    #[must_use]
    pub fn current() -> &'static Self {
        &RELEASED[RELEASED.len() - 1]
    }

    /// Every policy Whetstone has released, oldest first, each with its
    /// own defaults. The last is [`AssessmentPolicy::current`].
    ///
    /// ```
    /// use whetstone_core::{AssessmentPolicy, Job};
    ///
    /// let [policy_0, policy_1] = AssessmentPolicy::released() else {
    ///     unreachable!("policies 0 and 1 are released");
    /// };
    /// assert_eq!(policy_0.cut_offs(Job::GiveawayCheck).cut_offs().upper(), 0.80);
    /// assert_eq!(policy_1.cut_offs(Job::GiveawayCheck).cut_offs().upper(), 0.50);
    /// ```
    #[must_use]
    pub fn released() -> &'static [Self] {
        &RELEASED
    }

    /// The current policy with the `learner`'s own cut-offs for the jobs
    /// they list. Every other job keeps the policy's, and so does a job
    /// listed at the policy's values, so the policy is the learner's
    /// exactly where a cut-off differs. A job listed twice takes the last.
    ///
    /// ```
    /// use whetstone_core::{AssessmentPolicy, Band, CutOffs, CutOffsOrigin, Job, Probability};
    ///
    /// let lower_bar = CutOffs::new(0.55, 0.20).expect("from 0 to 1, lower below upper");
    /// let policy = AssessmentPolicy::with_cut_offs([(Job::QuestionCheck, lower_bar)]);
    /// let answer = Probability::new(0.6).expect("from 0 to 1");
    ///
    /// assert_eq!(policy.band(Job::QuestionCheck, answer), Band::ClearYes);
    /// assert_eq!(policy.band(Job::OptionRating, answer), Band::Unclear);
    /// assert_eq!(policy.origin(), CutOffsOrigin::Learner);
    /// assert_eq!(policy.version().get(), 1);
    /// ```
    #[must_use]
    pub fn with_cut_offs(learner: impl IntoIterator<Item = (Job, CutOffs)>) -> Self {
        let current = Self::current();
        let mut policy = current.clone();
        for (job, cut_offs) in learner {
            let (default, _) = current.cut_offs[job.index()];
            let origin = if cut_offs == default {
                CutOffsOrigin::Default
            } else {
                CutOffsOrigin::Learner
            };
            policy.cut_offs[job.index()] = (cut_offs, origin);
        }
        policy
    }

    /// Its version.
    #[must_use]
    pub fn version(&self) -> PolicyVersion {
        self.version
    }

    /// Whether its cut-offs have been checked against labelled examples.
    #[must_use]
    pub fn validation(&self) -> Validation {
        self.validation
    }

    /// The cut-offs that band `job`'s answers, and whose they are.
    #[must_use]
    pub fn cut_offs(&self, job: Job) -> AppliedCutOffs {
        let (cut_offs, origin) = self.cut_offs[job.index()];
        AppliedCutOffs {
            policy: self.version,
            cut_offs,
            origin,
        }
    }

    /// Whose cut-offs it applies: the learner's when any job's are their
    /// own, and then its outcomes aren't comparable with the policy's.
    #[must_use]
    pub fn origin(&self) -> CutOffsOrigin {
        if self.learner_jobs().next().is_some() {
            CutOffsOrigin::Learner
        } else {
            CutOffsOrigin::Default
        }
    }

    /// The jobs whose cut-offs are the learner's own, in [`Job::ALL`]
    /// order.
    pub fn learner_jobs(&self) -> impl Iterator<Item = Job> + '_ {
        Job::ALL
            .into_iter()
            .filter(|&job| self.cut_offs(job).origin == CutOffsOrigin::Learner)
    }

    /// The band of Jev's `answer` to a check of `job`: clear yes at or above
    /// the job's upper cut-off, clear no at or below its lower, and unclear
    /// in between.
    ///
    /// ```
    /// use whetstone_core::{AssessmentPolicy, Band, Job, Probability};
    ///
    /// let policy = AssessmentPolicy::current();
    /// let band = |p| policy.band(Job::OptionRating, Probability::new(p).expect("from 0 to 1"));
    ///
    /// assert_eq!(band(0.80), Band::ClearYes);
    /// assert_eq!(band(0.5), Band::Unclear);
    /// assert_eq!(band(0.20), Band::ClearNo);
    /// ```
    #[must_use]
    pub fn band(&self, job: Job, answer: Probability) -> Band {
        let (cut_offs, _) = self.cut_offs[job.index()];
        cut_offs.band(answer)
    }
}

impl CutOffs {
    /// The example cut-offs in Jev's documentation, 0.80 and 0.20, which
    /// policy 0 applies to every job, and policy 1 to every job but the
    /// giveaway check.
    pub const POLICY_0: Self = Self {
        upper: 0.80,
        lower: 0.20,
    };

    /// The cut-offs `upper` and `lower`, if each is a number from 0 to 1 and
    /// `lower` is below `upper`.
    pub fn new(upper: f64, lower: f64) -> Result<Self, InvalidCutOffs> {
        let from_0_to_1 = |value: f64| (0.0..=1.0).contains(&value);
        if !from_0_to_1(upper) {
            Err(InvalidCutOffs::Upper)
        } else if !from_0_to_1(lower) {
            Err(InvalidCutOffs::Lower)
        } else if lower >= upper {
            Err(InvalidCutOffs::NotBelow)
        } else {
            Ok(Self { upper, lower })
        }
    }

    /// The upper cut-off: an answer at or above it is clear yes.
    #[must_use]
    pub fn upper(self) -> f64 {
        self.upper
    }

    /// The lower cut-off: an answer at or below it is clear no.
    #[must_use]
    pub fn lower(self) -> f64 {
        self.lower
    }

    /// The band of Jev's `answer` under these cut-offs: clear yes at or
    /// above the upper, clear no at or below the lower, and unclear in
    /// between.
    #[must_use]
    pub fn band(self, answer: Probability) -> Band {
        let answer = answer.get();
        if answer >= self.upper {
            Band::ClearYes
        } else if answer <= self.lower {
            Band::ClearNo
        } else {
            Band::Unclear
        }
    }
}

impl AppliedCutOffs {
    /// The version of the policy whose cut-offs they are, or whose defaults
    /// the learner's replace.
    #[must_use]
    pub fn policy(self) -> PolicyVersion {
        self.policy
    }

    /// The job's cut-offs.
    #[must_use]
    pub fn cut_offs(self) -> CutOffs {
        self.cut_offs
    }

    /// Whether they are the policy's defaults or the learner's own.
    #[must_use]
    pub fn origin(self) -> CutOffsOrigin {
        self.origin
    }

    /// The band of Jev's `answer` under them.
    #[must_use]
    pub fn band(self, answer: Probability) -> Band {
        self.cut_offs.band(answer)
    }
}

/// As Settings and Inspect show them: `upper 0.55, lower 0.2`.
impl fmt::Display for CutOffs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "upper {}, lower {}", self.upper, self.lower)
    }
}

impl PolicyVersion {
    /// The version as a number.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

/// Its name, as Settings and Inspect show it: `question check`.
impl fmt::Display for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RelevanceGate => "relevance gate",
            Self::QuestionCheck => "question check",
            Self::GiveawayCheck => "giveaway check",
            Self::OptionRating => "option rating",
            Self::RationaleAndHintChecks => "rationale and hint checks",
            Self::ReasoningCriterion => "reasoning criterion",
            Self::ExplanationCheck => "explanation check",
        })
    }
}

impl fmt::Display for PolicyVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Job {
    /// Every job, in the order the policy lists them.
    pub const ALL: [Self; 7] = [
        Self::RelevanceGate,
        Self::QuestionCheck,
        Self::GiveawayCheck,
        Self::OptionRating,
        Self::RationaleAndHintChecks,
        Self::ReasoningCriterion,
        Self::ExplanationCheck,
    ];

    /// Its place in [`Job::ALL`].
    const fn index(self) -> usize {
        match self {
            Self::RelevanceGate => 0,
            Self::QuestionCheck => 1,
            Self::GiveawayCheck => 2,
            Self::OptionRating => 3,
            Self::RationaleAndHintChecks => 4,
            Self::ReasoningCriterion => 5,
            Self::ExplanationCheck => 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probability(value: f64) -> Probability {
        Probability::new(value).expect("from 0 to 1")
    }

    #[test]
    fn the_current_policy_is_policy_1_unvalidated() {
        let policy = AssessmentPolicy::current();

        assert_eq!(policy.version().get(), 1);
        assert_eq!(policy.validation(), Validation::Unvalidated);
    }

    /// Each band `job`'s answers fall in under `policy`, at and around
    /// `upper` and 0.20.
    fn assert_bands(policy: &AssessmentPolicy, job: Job, upper: f64) {
        for (value, band) in [
            (1.0, Band::ClearYes),
            (upper + 0.01, Band::ClearYes),
            (upper, Band::ClearYes),
            (upper - 0.01, Band::Unclear),
            (0.21, Band::Unclear),
            (0.20, Band::ClearNo),
            (0.19, Band::ClearNo),
            (0.0, Band::ClearNo),
        ] {
            assert_eq!(
                policy.band(job, probability(value)),
                band,
                "{job:?} {value}"
            );
        }
    }

    #[test]
    fn the_giveaway_check_is_clear_yes_from_0_50_and_clear_no_to_0_20_under_policy_1() {
        let policy = AssessmentPolicy::current();

        assert_bands(policy, Job::GiveawayCheck, 0.50);
        assert_eq!(
            policy.band(Job::GiveawayCheck, probability(0.55)),
            Band::ClearYes
        );
        assert_eq!(
            policy.cut_offs(Job::GiveawayCheck).cut_offs(),
            cut_offs(0.50, 0.20)
        );
    }

    #[test]
    fn every_other_job_is_clear_yes_from_0_80_and_clear_no_to_0_20_under_policy_1() {
        let policy = AssessmentPolicy::current();
        for job in Job::ALL
            .into_iter()
            .filter(|&job| job != Job::GiveawayCheck)
        {
            assert_bands(policy, job, 0.80);
            assert_eq!(
                policy.band(job, probability(0.55)),
                Band::Unclear,
                "{job:?}"
            );
        }
    }

    #[test]
    fn policy_0_stays_on_record_with_every_job_at_0_80_and_0_20() {
        let [policy_0, policy_1] = AssessmentPolicy::released() else {
            panic!("two policies released: {:?}", AssessmentPolicy::released());
        };

        assert_eq!(policy_0.version().get(), 0);
        assert_eq!(policy_0.validation(), Validation::Unvalidated);
        for job in Job::ALL {
            assert_bands(policy_0, job, 0.80);
            assert_eq!(policy_0.cut_offs(job).policy().get(), 0, "{job:?}");
        }
        assert_eq!(policy_1, AssessmentPolicy::current());
    }

    #[test]
    fn an_outcome_banded_under_policy_0_keeps_policy_0_s_version_and_cut_offs() {
        let policy_0 = &AssessmentPolicy::released()[0];

        let giveaway = policy_0.cut_offs(Job::GiveawayCheck);
        let outcome = policy_0.criterion_outcome(probability(0.55));

        assert_eq!(
            (giveaway.policy().get(), giveaway.cut_offs()),
            (0, cut_offs(0.80, 0.20))
        );
        assert_eq!(giveaway.band(probability(0.55)), Band::Unclear);
        assert_eq!(outcome.policy().get(), 0);
        assert_eq!(outcome.cut_offs().cut_offs(), cut_offs(0.80, 0.20));
        assert_ne!(
            outcome.cut_offs(),
            AssessmentPolicy::current().cut_offs(Job::ReasoningCriterion)
        );
    }

    fn cut_offs(upper: f64, lower: f64) -> CutOffs {
        CutOffs::new(upper, lower).expect("valid cut-offs")
    }

    #[test]
    fn cut_offs_are_numbers_from_0_to_1_with_lower_below_upper() {
        for (upper, lower) in [(0.55, 0.20), (1.0, 0.0), (0.5, 0.49), (1.0, 0.99)] {
            let valid = CutOffs::new(upper, lower).expect("valid");
            assert_eq!((valid.upper(), valid.lower()), (upper, lower));
        }
        for (upper, lower, invalid) in [
            (1.1, 0.2, InvalidCutOffs::Upper),
            (-0.1, -0.2, InvalidCutOffs::Upper),
            (f64::NAN, 0.2, InvalidCutOffs::Upper),
            (f64::INFINITY, 0.2, InvalidCutOffs::Upper),
            (0.8, -0.01, InvalidCutOffs::Lower),
            (0.8, f64::NAN, InvalidCutOffs::Lower),
            (0.8, 1.5, InvalidCutOffs::Lower),
            (0.5, 0.5, InvalidCutOffs::NotBelow),
            (0.2, 0.8, InvalidCutOffs::NotBelow),
        ] {
            assert_eq!(
                CutOffs::new(upper, lower),
                Err(invalid),
                "{upper} and {lower}"
            );
        }
    }

    #[test]
    fn without_the_learner_s_cut_offs_the_policy_is_policy_1() {
        let policy = AssessmentPolicy::with_cut_offs([]);

        assert_eq!(&policy, AssessmentPolicy::current());
        assert_eq!(policy.origin(), CutOffsOrigin::Default);
        assert_eq!(policy.learner_jobs().count(), 0);
        for job in Job::ALL {
            let applied = policy.cut_offs(job);
            let upper = if job == Job::GiveawayCheck {
                0.50
            } else {
                0.80
            };
            assert_eq!(applied.cut_offs(), cut_offs(upper, 0.20), "{job:?}");
            assert_eq!(applied.origin(), CutOffsOrigin::Default, "{job:?}");
            assert_eq!(applied.policy().get(), 1, "{job:?}");
        }
    }

    #[test]
    fn the_learner_s_cut_offs_band_their_job_and_every_other_job_keeps_policy_1_s() {
        let policy = AssessmentPolicy::with_cut_offs([(Job::QuestionCheck, cut_offs(0.55, 0.20))]);

        assert_eq!(policy.version().get(), 1);
        assert_eq!(policy.validation(), Validation::Unvalidated);
        assert_eq!(policy.origin(), CutOffsOrigin::Learner);
        assert_eq!(
            policy.learner_jobs().collect::<Vec<_>>(),
            [Job::QuestionCheck]
        );
        assert_eq!(
            policy.band(Job::QuestionCheck, probability(0.6)),
            Band::ClearYes
        );
        assert_eq!(
            policy.band(Job::QuestionCheck, probability(0.54)),
            Band::Unclear
        );
        let applied = policy.cut_offs(Job::QuestionCheck);
        assert_eq!(applied.cut_offs(), cut_offs(0.55, 0.20));
        assert_eq!(applied.origin(), CutOffsOrigin::Learner);
        for job in Job::ALL
            .into_iter()
            .filter(|&job| job != Job::QuestionCheck)
        {
            assert_eq!(
                policy.cut_offs(job),
                AssessmentPolicy::current().cut_offs(job),
                "{job:?}"
            );
        }
    }

    #[test]
    fn the_learner_s_question_check_leaves_the_giveaway_check_at_policy_1_s() {
        let raised = AssessmentPolicy::with_cut_offs([(Job::QuestionCheck, cut_offs(0.95, 0.20))]);
        let lowered = AssessmentPolicy::with_cut_offs([(Job::QuestionCheck, cut_offs(0.30, 0.10))]);

        for policy in [raised, lowered] {
            let giveaway = policy.cut_offs(Job::GiveawayCheck);
            assert_eq!(giveaway.cut_offs(), cut_offs(0.50, 0.20));
            assert_eq!(giveaway.origin(), CutOffsOrigin::Default);
            assert_eq!(
                policy.band(Job::GiveawayCheck, probability(0.4)),
                Band::Unclear
            );
            assert_eq!(
                policy.band(Job::GiveawayCheck, probability(0.55)),
                Band::ClearYes
            );
            assert_eq!(
                policy.learner_jobs().collect::<Vec<_>>(),
                [Job::QuestionCheck]
            );
        }
    }

    #[test]
    fn the_learner_s_giveaway_check_bands_no_giveaway_and_policy_0_s_values_are_their_own() {
        let policy = AssessmentPolicy::with_cut_offs([(Job::GiveawayCheck, cut_offs(0.80, 0.20))]);

        assert_eq!(
            policy.band(Job::GiveawayCheck, probability(0.55)),
            Band::Unclear
        );
        assert_eq!(policy.origin(), CutOffsOrigin::Learner);
        assert_eq!(
            policy.learner_jobs().collect::<Vec<_>>(),
            [Job::GiveawayCheck]
        );
        assert_eq!(policy.version().get(), 1);
    }

    #[test]
    fn every_job_can_take_the_learner_s_cut_offs() {
        let policy = AssessmentPolicy::with_cut_offs(Job::ALL.map(|job| (job, cut_offs(0.9, 0.1))));

        for job in Job::ALL {
            assert_eq!(
                policy.band(job, probability(0.85)),
                Band::Unclear,
                "{job:?}"
            );
            assert_eq!(policy.band(job, probability(0.1)), Band::ClearNo, "{job:?}");
            assert_eq!(policy.cut_offs(job).origin(), CutOffsOrigin::Learner);
        }
    }

    #[test]
    fn the_learner_s_cut_offs_at_policy_1_s_values_are_policy_1_s() {
        let policy = AssessmentPolicy::with_cut_offs([
            (Job::OptionRating, cut_offs(0.80, 0.20)),
            (Job::GiveawayCheck, cut_offs(0.50, 0.20)),
        ]);

        assert_eq!(&policy, AssessmentPolicy::current());
        assert_eq!(policy.origin(), CutOffsOrigin::Default);
    }

    #[test]
    fn a_cut_off_is_exact_with_no_rounding() {
        let policy = AssessmentPolicy::current();
        let just_under = f64::from_bits(0.80_f64.to_bits() - 1);
        let just_over = f64::from_bits(0.20_f64.to_bits() + 1);
        let giveaway_just_under = f64::from_bits(0.50_f64.to_bits() - 1);

        assert_eq!(
            policy.band(Job::QuestionCheck, probability(just_under)),
            Band::Unclear
        );
        assert_eq!(
            policy.band(Job::QuestionCheck, probability(just_over)),
            Band::Unclear
        );
        assert_eq!(
            policy.band(Job::GiveawayCheck, probability(giveaway_just_under)),
            Band::Unclear
        );
    }
}
