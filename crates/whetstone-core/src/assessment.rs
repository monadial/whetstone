//! Assessments: the stated outcomes of a learner's response, and the option
//! ratings they rest on.
//!
//! A response is assessed twice, separately, with no combined grade: its
//! choice from the question's option ratings, at once, and its reasoning,
//! if the learner wrote any, criterion by criterion from Jev's answers.
//! Reasoning is optional: a response with none is a choice alone, whose
//! reasoning isn't assessed. Every outcome carries the Jev
//! answer it came from and the cut-offs that banded it: the policy's
//! version, the job's cut-offs, and whether they are the policy's defaults
//! or the learner's own. An outcome is always one of its enum's states,
//! never a `bool`.

use std::fmt;

use crate::assessment_policy::{AppliedCutOffs, AssessmentPolicy, Band, Job, PolicyVersion};
use crate::jev::Probability;
use crate::question::OptionLetter;
use crate::question_check::{CandidateCheck, Question};

/// An outcome, with the Jev answer it came from and the cut-offs that
/// banded that answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Assessed<T> {
    outcome: T,
    answer: Probability,
    cut_offs: AppliedCutOffs,
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
/// reasoning, written or not written. Once submitted, it is final.
///
/// Its `Debug` output shows whether the reasoning is written, and its
/// length, never its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    choice: OptionLetter,
    reasoning: Reasoning,
}

/// A response's reasoning: writing it is optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reasoning {
    /// The learner wrote reasoning, which Jev assesses against each
    /// criterion of the rubric.
    Written(WrittenReasoning),
    /// The learner wrote none, or only whitespace: the response is a choice
    /// alone, and nothing assesses its reasoning.
    NotWritten,
}

/// Reasoning the learner wrote, as they wrote it: never empty or only
/// whitespace.
///
/// Its `Debug` output shows its length, never its text.
#[derive(Clone, PartialEq, Eq)]
pub struct WrittenReasoning(String);

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
        self.cut_offs.policy()
    }

    /// The cut-offs that banded the answer, and whether they are the
    /// policy's defaults or the learner's own.
    #[must_use]
    pub fn cut_offs(&self) -> AppliedCutOffs {
        self.cut_offs
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
        self.assessed(Job::OptionRating, outcome, answer)
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
        self.assessed(Job::ReasoningCriterion, outcome, answer)
    }

    /// `outcome`, which this policy banded from `answer` with `job`'s
    /// cut-offs.
    fn assessed<T>(&self, job: Job, outcome: T, answer: Probability) -> Assessed<T> {
        Assessed {
            outcome,
            answer,
            cut_offs: self.cut_offs(job),
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
            cut_offs: self.policy().cut_offs(Job::OptionRating),
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
            cut_offs: rating.cut_offs,
        }
    }
}

impl Response {
    /// The response choosing `choice`, with `reasoning` as the learner wrote
    /// it: not written when it is empty or only whitespace.
    #[must_use]
    pub fn new(choice: OptionLetter, reasoning: String) -> Self {
        let reasoning = if reasoning.trim().is_empty() {
            Reasoning::NotWritten
        } else {
            Reasoning::Written(WrittenReasoning(reasoning))
        };
        Self { choice, reasoning }
    }

    /// The selected choice.
    #[must_use]
    pub fn choice(&self) -> OptionLetter {
        self.choice
    }

    /// The reasoning, written or not written.
    #[must_use]
    pub fn reasoning(&self) -> &Reasoning {
        &self.reasoning
    }
}

impl Reasoning {
    /// The reasoning, if the learner wrote it.
    #[must_use]
    pub fn written(&self) -> Option<&WrittenReasoning> {
        match self {
            Self::Written(reasoning) => Some(reasoning),
            Self::NotWritten => None,
        }
    }
}

impl WrittenReasoning {
    /// The reasoning, as the learner wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Shows the reasoning's length, never the reasoning.
impl fmt::Debug for WrittenReasoning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrittenReasoning")
            .field("bytes", &self.0.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assessment_policy::{CutOffs, CutOffsOrigin};
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
    fn an_outcome_records_the_cut_offs_that_banded_it_and_whose_they_are() {
        let cut_offs = CutOffs::new(0.7, 0.3).expect("valid");
        let policy = AssessmentPolicy::with_cut_offs([
            (Job::ReasoningCriterion, cut_offs),
            (Job::OptionRating, cut_offs),
        ]);

        let criterion = policy.criterion_outcome(probability(0.75));
        let rating = policy.option_rating(probability(0.25));

        assert_eq!(criterion.outcome(), CriterionOutcome::Met);
        assert_eq!(rating.outcome(), OptionRating::NotSupported);
        for applied in [criterion.cut_offs(), rating.cut_offs()] {
            assert_eq!(applied.cut_offs(), cut_offs);
            assert_eq!(applied.origin(), CutOffsOrigin::Learner);
            assert_eq!(applied.policy().get(), 0);
        }
        let default = AssessmentPolicy::current().criterion_outcome(probability(0.75));
        assert_eq!(default.outcome(), CriterionOutcome::NeedsReview);
        assert_eq!(
            default.cut_offs(),
            AssessmentPolicy::current().cut_offs(Job::ReasoningCriterion)
        );
    }

    #[test]
    fn a_shown_question_s_option_ratings_and_choice_carry_the_cut_offs_it_was_checked_with() {
        let candidate = candidate("domain", 0, OptionLetter::A);
        let answers = passing(&candidate);
        let policy = AssessmentPolicy::with_cut_offs([(
            Job::OptionRating,
            CutOffs::new(0.85, 0.15).expect("valid"),
        )]);
        let question = show_rule(
            candidate,
            QuestionCheckAnswers::new(answers, depth(1.0)),
            &policy,
        )
        .expect("shown");

        let applied = policy.cut_offs(Job::OptionRating);
        assert_eq!(question.option_rating(OptionLetter::B).cut_offs(), applied);
        assert_eq!(question.choice_outcome(OptionLetter::A).cut_offs(), applied);
        assert_eq!(applied.origin(), CutOffsOrigin::Learner);
    }

    #[test]
    fn a_response_is_a_choice_and_its_reasoning_as_written() {
        let response = Response::new(OptionLetter::A, " The key dedupes.\n".to_owned());

        assert_eq!(response.choice(), OptionLetter::A);
        let Reasoning::Written(reasoning) = response.reasoning() else {
            panic!("reasoning is written: {response:?}");
        };
        assert_eq!(reasoning.as_str(), " The key dedupes.\n");
        assert_eq!(response.reasoning().written(), Some(reasoning));
    }

    #[test]
    fn blank_or_whitespace_only_reasoning_is_not_written() {
        for reasoning in ["", " ", " \n\t\r\n"] {
            let response = Response::new(OptionLetter::B, reasoning.to_owned());

            assert_eq!(response.choice(), OptionLetter::B);
            assert_eq!(
                response.reasoning(),
                &Reasoning::NotWritten,
                "{reasoning:?}"
            );
            assert_eq!(response.reasoning().written(), None);
        }
    }

    #[test]
    fn debug_output_never_shows_the_reasoning() {
        let response = Response::new(OptionLetter::C, "The key dedupes.".to_owned());

        let debug = format!("{response:?}");

        assert!(!debug.contains("dedupes"), "{debug}");
        assert!(debug.contains("choice: C"), "{debug}");
        assert!(debug.contains("Written"), "{debug}");
        let debug = format!("{:?}", Response::new(OptionLetter::C, String::new()));
        assert!(debug.contains("NotWritten"), "{debug}");
    }
}
