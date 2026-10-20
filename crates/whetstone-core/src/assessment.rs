//! Assessments: the stated outcomes of a learner's response, and the option
//! ratings they rest on.
//!
//! A response is assessed twice, separately, with no combined grade: its
//! choice from the question's option ratings, at once, and its reasoning
//! criterion by criterion from Jev's answers. Every outcome carries the Jev
//! answer it came from and the version of the policy that banded it. An
//! outcome is always one of its enum's states, never a `bool`.

use std::fmt;

use crate::assessment_policy::{AssessmentPolicy, Band, Job, PolicyVersion};
use crate::jev::Probability;
use crate::question::OptionLetter;
use crate::question_check::{CandidateCheck, Question};

/// An outcome, with the Jev answer it came from and the version of the
/// policy that banded that answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Assessed<T> {
    outcome: T,
    answer: Probability,
    policy: PolicyVersion,
}

/// One option's rating, decided in its question's check before the learner
/// sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionRating {
    /// Jev clearly says the option correctly answers the question.
    Supported,
    /// Jev clearly says it doesn't.
    NotSupported,
    /// Jev's answer is unclear.
    NeedsReview,
}

/// The outcome of the learner's selected choice. It is always definite: the
/// show rule only shows a question whose options are all clearly rated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChoiceOutcome {
    /// The chosen option is the one Jev clearly supports.
    Supported,
    /// Jev clearly doesn't support the chosen option.
    NotSupported,
}

/// The outcome of the learner's reasoning against one criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CriterionOutcome {
    /// Jev clearly says the reasoning covers the criterion.
    Met,
    /// Jev clearly says it doesn't.
    NotMet,
    /// Jev couldn't tell whether the reasoning covers it.
    NeedsReview,
}

/// The learner's response to a question: their selected choice and their
/// written reasoning. Once submitted, it is final.
///
/// Its `Debug` output shows the reasoning's length, never its text.
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    choice: OptionLetter,
    reasoning: String,
}

/// A response without written reasoning: there is none, or only whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a response needs written reasoning")]
pub struct MissingReasoning;

impl<T: Copy> Assessed<T> {
    /// The outcome.
    #[must_use]
    pub fn outcome(&self) -> T {
        self.outcome
    }

    /// The Jev answer it came from, as returned.
    #[must_use]
    pub fn answer(&self) -> Probability {
        self.answer
    }

    /// The version of the policy that banded the answer.
    #[must_use]
    pub fn policy(&self) -> PolicyVersion {
        self.policy
    }
}

impl AssessmentPolicy {
    /// An option's rating from Jev's `answer` to whether it correctly answers
    /// its question: Supported when clear yes, Not supported when clear no,
    /// and Needs review when unclear.
    #[must_use]
    pub fn option_rating(&self, answer: Probability) -> Assessed<OptionRating> {
        let outcome = match self.band(Job::OptionRating, answer) {
            Band::ClearYes => OptionRating::Supported,
            Band::ClearNo => OptionRating::NotSupported,
            Band::Unclear => OptionRating::NeedsReview,
        };
        self.assessed(outcome, answer)
    }

    /// A criterion's outcome from Jev's `answer` to whether the learner's
    /// reasoning meets it: met when clear yes, not met when clear no, and
    /// needs review when unclear.
    #[must_use]
    pub fn criterion_outcome(&self, answer: Probability) -> Assessed<CriterionOutcome> {
        let outcome = match self.band(Job::ReasoningCriterion, answer) {
            Band::ClearYes => CriterionOutcome::Met,
            Band::ClearNo => CriterionOutcome::NotMet,
            Band::Unclear => CriterionOutcome::NeedsReview,
        };
        self.assessed(outcome, answer)
    }

    /// `outcome`, which this policy banded from `answer`.
    fn assessed<T>(&self, outcome: T, answer: Probability) -> Assessed<T> {
        Assessed {
            outcome,
            answer,
            policy: self.version(),
        }
    }
}

impl Question {
    /// The option `letter`'s rating, from its question check. A shown
    /// question's answer key is Supported and its other options are Not
    /// supported: the show rule passed it on exactly those bands.
    #[must_use]
    pub fn option_rating(&self, letter: OptionLetter) -> Assessed<OptionRating> {
        let outcome = if letter == self.answer_key() {
            OptionRating::Supported
        } else {
            OptionRating::NotSupported
        };
        Assessed {
            outcome,
            answer: self
                .answers()
                .get(CandidateCheck::Option(letter))
                .expect("the show rule shows only a question with every option rated"),
            policy: self.policy(),
        }
    }

    /// The outcome of choosing `chosen`, at once, from the option ratings:
    /// Supported when Jev supports the chosen option, Not supported when it
    /// doesn't. It is never Needs review: every option of a shown question is
    /// clearly rated, and the answer key is the only one Supported. It
    /// carries the chosen option's answer.
    #[must_use]
    pub fn choice_outcome(&self, chosen: OptionLetter) -> Assessed<ChoiceOutcome> {
        let rating = self.option_rating(chosen);
        let outcome = if chosen == self.answer_key() {
            ChoiceOutcome::Supported
        } else {
            ChoiceOutcome::NotSupported
        };
        Assessed {
            outcome,
            answer: rating.answer,
            policy: rating.policy,
        }
    }
}

impl Response {
    /// The response choosing `choice`, with `reasoning`, if the learner wrote
    /// any.
    pub fn new(choice: OptionLetter, reasoning: String) -> Result<Self, MissingReasoning> {
        if reasoning.trim().is_empty() {
            return Err(MissingReasoning);
        }
        Ok(Self { choice, reasoning })
    }

    /// The selected choice.
    #[must_use]
    pub fn choice(&self) -> OptionLetter {
        self.choice
    }

    /// The written reasoning, as the learner wrote it.
    #[must_use]
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }
}

/// Shows the choice and the reasoning's length, never the reasoning.
impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("choice", &self.choice)
            .field("reasoning_bytes", &self.reasoning.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::question_check::tests::{candidate, depth, passing, probability};
    use crate::question_check::{QuestionCheckAnswers, show_rule};

    /// A question whose answer key is `answer_key`, with options rated
    /// 0.95 for its answer key and 0.03 for the others.
    fn question(answer_key: OptionLetter) -> Question {
        let candidate = candidate("domain", 0, answer_key);
        let mut answers = passing(&candidate);
        for letter in OptionLetter::ALL {
            let answer = if letter == answer_key { 0.95 } else { 0.03 };
            answers.push((CandidateCheck::Option(letter), probability(answer)));
        }
        show_rule(
            candidate,
            QuestionCheckAnswers::new(answers, depth(1.0)),
            AssessmentPolicy::current(),
        )
        .expect("shown")
    }

    #[test]
    fn choosing_the_option_jev_supports_is_supported() {
        let outcome = question(OptionLetter::B).choice_outcome(OptionLetter::B);

        assert_eq!(outcome.outcome(), ChoiceOutcome::Supported);
        assert_eq!(outcome.answer(), probability(0.95));
        assert_eq!(outcome.policy(), AssessmentPolicy::current().version());
    }

    #[test]
    fn choosing_an_option_jev_does_not_support_is_not_supported() {
        let question = question(OptionLetter::B);
        for chosen in [OptionLetter::A, OptionLetter::C] {
            let outcome = question.choice_outcome(chosen);

            assert_eq!(outcome.outcome(), ChoiceOutcome::NotSupported, "{chosen}");
            assert_eq!(outcome.answer(), probability(0.03), "{chosen}");
            assert_eq!(outcome.policy(), AssessmentPolicy::current().version());
        }
    }

    #[test]
    fn a_shown_questions_options_are_rated_from_its_check() {
        let question = question(OptionLetter::C);

        for (letter, outcome, answer) in [
            (OptionLetter::A, OptionRating::NotSupported, 0.03),
            (OptionLetter::B, OptionRating::NotSupported, 0.03),
            (OptionLetter::C, OptionRating::Supported, 0.95),
        ] {
            let rating = question.option_rating(letter);

            assert_eq!(rating.outcome(), outcome, "{letter}");
            assert_eq!(rating.answer(), probability(answer), "{letter}");
            assert_eq!(rating.policy(), AssessmentPolicy::current().version());
        }
    }

    #[test]
    fn an_option_rating_is_supported_not_supported_or_needs_review() {
        let policy = AssessmentPolicy::current();
        for (answer, outcome) in [
            (0.80, OptionRating::Supported),
            (0.79, OptionRating::NeedsReview),
            (0.21, OptionRating::NeedsReview),
            (0.20, OptionRating::NotSupported),
        ] {
            let rating = policy.option_rating(probability(answer));

            assert_eq!(rating.outcome(), outcome, "{answer}");
            assert_eq!(rating.answer(), probability(answer));
            assert_eq!(rating.policy(), policy.version());
        }
    }

    #[test]
    fn a_criterion_is_met_not_met_or_needs_review() {
        let policy = AssessmentPolicy::current();
        for (answer, outcome) in [
            (1.0, CriterionOutcome::Met),
            (0.80, CriterionOutcome::Met),
            (0.79, CriterionOutcome::NeedsReview),
            (0.5, CriterionOutcome::NeedsReview),
            (0.21, CriterionOutcome::NeedsReview),
            (0.20, CriterionOutcome::NotMet),
            (0.0, CriterionOutcome::NotMet),
        ] {
            let assessed = policy.criterion_outcome(probability(answer));

            assert_eq!(assessed.outcome(), outcome, "{answer}");
            assert_eq!(assessed.answer(), probability(answer));
            assert_eq!(assessed.policy(), policy.version());
        }
    }

    #[test]
    fn a_response_is_a_choice_and_written_reasoning() {
        let response = Response::new(OptionLetter::A, "The key dedupes.".to_owned())
            .expect("reasoning is written");

        assert_eq!(response.choice(), OptionLetter::A);
        assert_eq!(response.reasoning(), "The key dedupes.");
        for reasoning in ["", " \n\t"] {
            assert_eq!(
                Response::new(OptionLetter::A, reasoning.to_owned()),
                Err(MissingReasoning)
            );
        }
    }

    #[test]
    fn debug_output_never_shows_the_reasoning() {
        let response = Response::new(OptionLetter::C, "The key dedupes.".to_owned())
            .expect("reasoning is written");

        let debug = format!("{response:?}");

        assert!(!debug.contains("dedupes"), "{debug}");
        assert!(debug.contains("choice: C"), "{debug}");
    }
}
