//! The question check: Jev's check of a candidate before it is shown, and
//! the show rule that turns its answers into a question, or withholds the
//! candidate.
//!
//! [`Candidate::checks`] lists the yes/no questions the check asks. Jev's
//! answers come back as [`QuestionCheckAnswers`], as returned, and
//! [`show_rule`] decides under the [`AssessmentPolicy`]. Only a question
//! that passes is ever shown.

use std::collections::BTreeMap;
use std::fmt;

use crate::area::AreaId;
use crate::assessment_policy::{AssessmentPolicy, Band, Job, PolicyVersion};
use crate::jev::Probability;
use crate::question::{Candidate, CandidateId, Citation, Criterion, OptionLetter, QuestionType};

/// One yes/no question of a candidate's question check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CandidateCheck {
    /// Option rating: given the evidence, this option correctly answers the
    /// question.
    Option(OptionLetter),
}

/// Jev's answers to a candidate's question check, each the probability of
/// yes, exactly as returned.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuestionCheckAnswers {
    yes_no: BTreeMap<CandidateCheck, Probability>,
}

/// A candidate that passed its question check, with Jev's answers and the
/// version of the policy that passed it. Only questions are ever shown.
///
/// Its `Debug` output shows no question text.
#[derive(Clone, PartialEq)]
pub struct Question {
    candidate: Candidate,
    answers: QuestionCheckAnswers,
    policy: PolicyVersion,
}

/// A candidate the show rule withheld, and the checks that were not as the
/// rule needs them. It is never shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withheld {
    id: CandidateId,
    failed: Vec<CandidateCheck>,
}

impl Candidate {
    /// The yes/no questions its question check asks Jev, in order: one
    /// option rating per option.
    #[must_use]
    pub fn checks(&self) -> Vec<CandidateCheck> {
        match self.question_type() {
            QuestionType::SingleChoice => OptionLetter::ALL
                .into_iter()
                .map(CandidateCheck::Option)
                .collect(),
        }
    }
}

impl CandidateCheck {
    /// The job whose cut-offs band its answer.
    #[must_use]
    pub fn job(self) -> Job {
        match self {
            Self::Option(_) => Job::OptionRating,
        }
    }
}

impl QuestionCheckAnswers {
    /// Jev's `answers`, each to one check. A check answered twice keeps its
    /// last answer.
    #[must_use]
    pub fn new(answers: impl IntoIterator<Item = (CandidateCheck, Probability)>) -> Self {
        Self {
            yes_no: answers.into_iter().collect(),
        }
    }

    /// Jev's answer to `check`, if it was asked.
    #[must_use]
    pub fn get(&self, check: CandidateCheck) -> Option<Probability> {
        self.yes_no.get(&check).copied()
    }

    /// Every answer, by check, in [`CandidateCheck`] order.
    pub fn iter(&self) -> impl Iterator<Item = (CandidateCheck, Probability)> + '_ {
        self.yes_no.iter().map(|(&check, &answer)| (check, answer))
    }
}

/// The show rule: `candidate` becomes a question only when Jev's `answers`
/// to its question check are what its question type needs, under `policy`.
/// Anything unclear fails it, and so does a check left unanswered.
///
/// For a single-choice question: exactly one option rating is clear yes and
/// it is the answer key, and the other two are clear no.
///
/// The withheld candidate names every check that failed, in
/// [`Candidate::checks`] order.
pub fn show_rule(
    candidate: Candidate,
    answers: QuestionCheckAnswers,
    policy: &AssessmentPolicy,
) -> Result<Question, Withheld> {
    let band = |check: CandidateCheck| {
        answers
            .get(check)
            .map(|answer| policy.band(check.job(), answer))
    };
    let failed: Vec<CandidateCheck> = candidate
        .checks()
        .into_iter()
        .filter(|&check| band(check) != Some(needed(&candidate, check)))
        .collect();
    if failed.is_empty() {
        Ok(Question {
            candidate,
            answers,
            policy: policy.version(),
        })
    } else {
        Err(Withheld {
            id: candidate.id().clone(),
            failed,
        })
    }
}

/// The band the show rule needs for `candidate`'s `check`.
fn needed(candidate: &Candidate, check: CandidateCheck) -> Band {
    match check {
        CandidateCheck::Option(letter) if letter == candidate.answer_key() => Band::ClearYes,
        CandidateCheck::Option(_) => Band::ClearNo,
    }
}

impl Question {
    /// What identifies it: its area and its place in the order written.
    #[must_use]
    pub fn id(&self) -> &CandidateId {
        self.candidate.id()
    }

    /// The area it is about.
    #[must_use]
    pub fn area(&self) -> &AreaId {
        self.candidate.area()
    }

    /// Its question type.
    #[must_use]
    pub fn question_type(&self) -> QuestionType {
        self.candidate.question_type()
    }

    /// The question itself.
    #[must_use]
    pub fn stem(&self) -> &str {
        self.candidate.stem()
    }

    /// What the option `letter` says.
    #[must_use]
    pub fn option(&self, letter: OptionLetter) -> &str {
        self.candidate.option(letter)
    }

    /// Why the option `letter` is right or wrong, as the generator wrote it.
    #[must_use]
    pub fn rationale(&self, letter: OptionLetter) -> &str {
        self.candidate.rationale(letter)
    }

    /// The option the generator marks as correct, which Jev rated the only
    /// clearly supported one.
    #[must_use]
    pub fn answer_key(&self) -> OptionLetter {
        self.candidate.answer_key()
    }

    /// What the question is getting at.
    #[must_use]
    pub fn why_it_matters(&self) -> &str {
        self.candidate.why_it_matters()
    }

    /// The criteria written reasoning is assessed against: 1 to 3 of them.
    #[must_use]
    pub fn rubric(&self) -> &[Criterion] {
        self.candidate.rubric()
    }

    /// The lines of the evidence it rests on.
    #[must_use]
    pub fn citations(&self) -> &[Citation] {
        self.candidate.citations()
    }

    /// Jev's answers to its question check, as returned.
    #[must_use]
    pub fn answers(&self) -> &QuestionCheckAnswers {
        &self.answers
    }

    /// The version of the policy that passed it.
    #[must_use]
    pub fn policy(&self) -> PolicyVersion {
        self.policy
    }
}

/// Shows its candidate, without text, and Jev's answers.
impl fmt::Debug for Question {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Question")
            .field("candidate", &self.candidate)
            .field("answers", &self.answers)
            .field("policy", &self.policy)
            .finish()
    }
}

impl Withheld {
    /// The candidate withheld.
    #[must_use]
    pub fn id(&self) -> &CandidateId {
        &self.id
    }

    /// The checks that were not as the show rule needs them, in
    /// [`Candidate::checks`] order.
    #[must_use]
    pub fn failed(&self) -> &[CandidateCheck] {
        &self.failed
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::question::plain_checks;
    use crate::question::tests::{area, evidence, written};

    pub(crate) fn probability(value: f64) -> Probability {
        Probability::new(value).expect("from 0 to 1")
    }

    /// A candidate in `area_id` whose answer key is `answer_key`, written in
    /// place `written_at` (earlier places are dropped as malformed).
    pub(crate) fn candidate(
        area_id: &str,
        written_at: usize,
        answer_key: OptionLetter,
    ) -> Candidate {
        let mut batch: Vec<_> = (0..written_at)
            .map(|_| {
                let mut malformed = written(answer_key);
                malformed.options.clear();
                malformed
            })
            .collect();
        batch.push(written(answer_key));
        plain_checks(&area(area_id), batch, &evidence())
            .into_passed()
            .pop()
            .expect("a well-formed candidate passes")
    }

    /// Answers rating options A, B and C at `a`, `b` and `c`.
    pub(crate) fn options(a: f64, b: f64, c: f64) -> Vec<(CandidateCheck, Probability)> {
        OptionLetter::ALL
            .into_iter()
            .map(CandidateCheck::Option)
            .zip([a, b, c].map(probability))
            .collect()
    }

    fn show(
        answer_key: OptionLetter,
        answers: Vec<(CandidateCheck, Probability)>,
    ) -> Result<Question, Withheld> {
        show_rule(
            candidate("domain", 0, answer_key),
            QuestionCheckAnswers::new(answers),
            AssessmentPolicy::current(),
        )
    }

    fn failed(result: Result<Question, Withheld>) -> Vec<CandidateCheck> {
        result.expect_err("withheld").failed().to_vec()
    }

    #[test]
    fn a_single_choice_question_check_rates_each_option() {
        assert_eq!(
            candidate("domain", 0, OptionLetter::A).checks(),
            [
                CandidateCheck::Option(OptionLetter::A),
                CandidateCheck::Option(OptionLetter::B),
                CandidateCheck::Option(OptionLetter::C),
            ]
        );
        assert_eq!(
            CandidateCheck::Option(OptionLetter::B).job(),
            Job::OptionRating
        );
    }

    #[test]
    fn the_answer_key_clearly_supported_and_the_others_clearly_not_is_shown() {
        let question = show(OptionLetter::B, options(0.2, 0.8, 0.0)).expect("shown");

        assert_eq!(question.id().written(), 0);
        assert_eq!(question.area(), &area("domain"));
        assert_eq!(question.question_type(), QuestionType::SingleChoice);
        assert_eq!(question.stem(), "Why does the retry reuse the key?");
        assert_eq!(question.option(OptionLetter::A), "Option A");
        assert_eq!(question.rationale(OptionLetter::B), "Rationale B");
        assert_eq!(question.answer_key(), OptionLetter::B);
        assert_eq!(question.why_it_matters(), "A retry must not charge twice.");
        assert_eq!(question.rubric().len(), 2);
        assert_eq!(question.citations().len(), 1);
        assert_eq!(question.policy(), AssessmentPolicy::current().version());
        assert_eq!(
            question
                .answers()
                .get(CandidateCheck::Option(OptionLetter::B)),
            Some(probability(0.8))
        );
        assert_eq!(
            question.answers().iter().collect::<Vec<_>>(),
            options(0.2, 0.8, 0.0)
        );
    }

    #[test]
    fn two_options_clearly_supported_are_withheld() {
        assert_eq!(
            failed(show(OptionLetter::A, options(0.9, 0.85, 0.1))),
            [CandidateCheck::Option(OptionLetter::B)]
        );
    }

    #[test]
    fn no_option_clearly_supported_is_withheld() {
        assert_eq!(
            failed(show(OptionLetter::A, options(0.1, 0.0, 0.2))),
            [CandidateCheck::Option(OptionLetter::A)]
        );
    }

    #[test]
    fn an_unclear_option_is_withheld() {
        for (answers, unclear) in [
            (options(0.79, 0.1, 0.1), OptionLetter::A),
            (options(0.9, 0.21, 0.1), OptionLetter::B),
            (options(0.9, 0.0, 0.5), OptionLetter::C),
        ] {
            assert_eq!(
                failed(show(OptionLetter::A, answers)),
                [CandidateCheck::Option(unclear)]
            );
        }
    }

    #[test]
    fn an_answer_key_jev_disagrees_with_is_withheld() {
        assert_eq!(
            failed(show(OptionLetter::C, options(0.95, 0.05, 0.05))),
            [
                CandidateCheck::Option(OptionLetter::A),
                CandidateCheck::Option(OptionLetter::C),
            ]
        );
    }

    #[test]
    fn an_option_left_unanswered_is_withheld() {
        let mut answers = options(0.9, 0.1, 0.1);
        answers.pop();

        assert_eq!(
            failed(show(OptionLetter::A, answers)),
            [CandidateCheck::Option(OptionLetter::C)]
        );
    }

    #[test]
    fn a_withheld_candidate_names_itself() {
        let withheld = show_rule(
            candidate("observability", 2, OptionLetter::A),
            QuestionCheckAnswers::new(options(0.5, 0.5, 0.5)),
            AssessmentPolicy::current(),
        )
        .expect_err("withheld");

        assert_eq!(withheld.id().area(), &area("observability"));
        assert_eq!(withheld.id().written(), 2);
    }

    #[test]
    fn debug_output_never_shows_a_questions_text() {
        let question = show(OptionLetter::A, options(0.9, 0.1, 0.1)).expect("shown");
        let debug = format!("{question:?}");

        for text in [
            "retry reuse",
            "Option A",
            "Rationale",
            "charge twice",
            "Look at",
        ] {
            assert!(!debug.contains(text), "{debug}");
        }
    }
}
