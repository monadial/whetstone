//! The assessment policy: the versioned cut-offs that turn Jev's
//! probabilities into clear yes, unclear or clear no, for each kind of check.
//!
//! The policy is built in and cannot be configured. Changing a cut-off is a
//! recorded decision that bumps the version (`CODING_STANDARDS.md` rule 38), so
//! every outcome can be traced to the cut-offs that produced it.

use std::fmt;

use crate::jev::Probability;

/// The cut-offs Whetstone applies to Jev's answers, with their version and
/// whether they have been checked against labelled examples.
#[derive(Debug, Clone, PartialEq)]
pub struct AssessmentPolicy {
    version: PolicyVersion,
    validation: Validation,
    /// Each job's cut-offs, in [`Job::ALL`] order.
    cut_offs: [CutOffs; Job::ALL.len()],
}

/// An assessment policy's version. Every outcome carries the version of the
/// policy that banded it.
///
/// ```
/// use whetstone_core::AssessmentPolicy;
///
/// assert_eq!(AssessmentPolicy::current().version().to_string(), "0");
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
    /// A candidate's checks 1–5: answerable from the evidence, about its
    /// area, needs this change, doesn't give the answer away, and each
    /// criterion.
    QuestionCheck,
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
/// at or below `lower` is clear no.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CutOffs {
    upper: f64,
    lower: f64,
}

/// The example cut-offs in Jev's documentation, which policy 0 applies to
/// every job.
const EXAMPLE_CUT_OFFS: CutOffs = CutOffs {
    upper: 0.80,
    lower: 0.20,
};

/// Policy 0: unvalidated, with every job at 0.80 and 0.20.
static POLICY_0: AssessmentPolicy = AssessmentPolicy {
    version: PolicyVersion(0),
    validation: Validation::Unvalidated,
    cut_offs: [EXAMPLE_CUT_OFFS; Job::ALL.len()],
};

impl AssessmentPolicy {
    /// The policy this release applies: version 0, unvalidated, with every
    /// job's upper cut-off at 0.80 and its lower at 0.20.
    #[must_use]
    pub fn current() -> &'static Self {
        &POLICY_0
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
        let CutOffs { upper, lower } = self.cut_offs[job.index()];
        let answer = answer.get();
        if answer >= upper {
            Band::ClearYes
        } else if answer <= lower {
            Band::ClearNo
        } else {
            Band::Unclear
        }
    }
}

impl PolicyVersion {
    /// The version as a number.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PolicyVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Job {
    /// Every job, in the order the policy lists them.
    pub const ALL: [Self; 6] = [
        Self::RelevanceGate,
        Self::QuestionCheck,
        Self::OptionRating,
        Self::RationaleAndHintChecks,
        Self::ReasoningCriterion,
        Self::ExplanationCheck,
    ];

    /// Its place in [`Job::ALL`].
    fn index(self) -> usize {
        match self {
            Self::RelevanceGate => 0,
            Self::QuestionCheck => 1,
            Self::OptionRating => 2,
            Self::RationaleAndHintChecks => 3,
            Self::ReasoningCriterion => 4,
            Self::ExplanationCheck => 5,
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
    fn policy_0_is_unvalidated() {
        let policy = AssessmentPolicy::current();

        assert_eq!(policy.version().get(), 0);
        assert_eq!(policy.validation(), Validation::Unvalidated);
    }

    #[test]
    fn every_job_is_clear_yes_from_0_80_and_clear_no_to_0_20_under_policy_0() {
        let policy = AssessmentPolicy::current();
        for job in Job::ALL {
            for (value, band) in [
                (1.0, Band::ClearYes),
                (0.81, Band::ClearYes),
                (0.80, Band::ClearYes),
                (0.79, Band::Unclear),
                (0.5, Band::Unclear),
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
    }

    #[test]
    fn a_cut_off_is_exact_with_no_rounding() {
        let policy = AssessmentPolicy::current();
        let just_under = f64::from_bits(0.80_f64.to_bits() - 1);
        let just_over = f64::from_bits(0.20_f64.to_bits() + 1);

        assert_eq!(
            policy.band(Job::QuestionCheck, probability(just_under)),
            Band::Unclear
        );
        assert_eq!(
            policy.band(Job::QuestionCheck, probability(just_over)),
            Band::Unclear
        );
    }
}
