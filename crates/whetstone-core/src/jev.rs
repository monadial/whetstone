//! Jev's answers, stored as Jev returns them (`CODING_STANDARDS.md` rule 38).
//!
//! Jev is the evaluator: it returns probabilities, never prose. Nothing here
//! rounds or renormalises an answer. Turning one into clear yes, unclear or
//! clear no is the [`AssessmentPolicy`](crate::AssessmentPolicy)'s job.

/// Jev's answer to a yes/no question: the probability of yes, from 0 to 1,
/// exactly as returned.
///
/// ```
/// use whetstone_core::Probability;
///
/// assert_eq!(Probability::new(0.8)?.get(), 0.8);
/// assert!(Probability::new(1.01).is_err());
/// # Ok::<(), whetstone_core::InvalidProbability>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Probability(f64);

/// A number Jev returned that is not a probability: below 0, above 1 or not
/// a number. A reply holding one is invalid, never coerced (rule 37).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("{0} is not a probability: it must be from 0 to 1")]
pub struct InvalidProbability(f64);

impl Probability {
    /// The probability `value`, if it is one: from 0 to 1, both included.
    pub fn new(value: f64) -> Result<Self, InvalidProbability> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidProbability(value))
        }
    }

    /// The probability as a number from 0 to 1.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probability_is_from_0_to_1_and_kept_as_returned() {
        for value in [0.0, 0.2, 0.57, 0.8, 1.0] {
            let probability = Probability::new(value).expect("from 0 to 1");

            assert!(probability.get().to_bits() == value.to_bits(), "{value}");
        }
    }

    #[test]
    fn anything_else_is_not_a_probability() {
        for value in [-0.01, 1.01, -1.0, 2.0, f64::NAN, f64::INFINITY] {
            assert!(Probability::new(value).is_err(), "{value}");
        }
    }
}
