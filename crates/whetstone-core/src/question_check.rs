//! The question check: Jev's check of a candidate before it is shown, and
//! the show rule that turns its answers into a question, or withholds the
//! candidate.
//!
//! [`Candidate::checks`] lists the yes/no questions the check asks: whether
//! the candidate is answerable from the evidence, about its area, needs this
//! change and doesn't give the answer away; whether each criterion is fair,
//! and whether it asks only for what the question needs; each option's
//! rating; whether each rationale and the "why it matters" line hold up;
//! and whether the hint points to the evidence without revealing the answer
//! key. It also scores the candidate's [`Depth`], which orders questions
//! and never passes or fails one. Jev's answers come back as
//! [`QuestionCheckAnswers`], as returned, and [`show_rule`] decides under
//! the [`AssessmentPolicy`]. Only a question that passes is ever shown.

use std::collections::BTreeMap;
use std::fmt;

use crate::area::AreaId;
use crate::assessment_policy::{AssessmentPolicy, Band, Job};
use crate::jev::Probability;
use crate::question::{
    Candidate, CandidateId, Citation, Criterion, Hint, OptionLetter, QuestionType,
};

/// One yes/no question of a candidate's question check. They order as the
/// check asks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CandidateCheck {
    /// Check 1: it is answerable from the evidence that was sent.
    Answerable,
    /// Check 2: it is about its area.
    AboutArea,
    /// Check 3: it needs this change rather than general knowledge.
    NeedsChange,
    /// Check 4: its stem and options don't give the answer away.
    NoGiveaway,
    /// Check 5, for the criterion at this place in the rubric, counting from
    /// 0: it follows from the evidence and the answer key, and can be judged
    /// from written reasoning.
    Criterion(usize),
    /// Check 6, for the criterion at this place in the rubric, counting from
    /// 0: it asks only for what a sufficient answer to the stem needs, so a
    /// learner who understands the change could meet it in their own words.
    CriterionMinimum(usize),
    /// Option rating: given the evidence, this option correctly answers the
    /// question.
    Option(OptionLetter),
    /// This option's rationale is supported by the evidence.
    Rationale(OptionLetter),
    /// The "why it matters" line is supported by the evidence.
    WhyItMatters,
    /// The hint points to the evidence.
    HintPointsToEvidence,
    /// The hint doesn't reveal the answer key.
    HintHidesAnswerKey,
}

/// Jev's answers to a candidate's question check, exactly as returned: each
/// yes/no answer the probability of yes, and its depth.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionCheckAnswers {
    yes_no: BTreeMap<CandidateCheck, Probability>,
    depth: Depth,
}

/// How much understanding of the change a candidate needs, as Jev scores it
/// over three ordered levels: its distribution over the levels and its
/// expected value, exactly as returned. Nothing renormalises the
/// distribution, and Jev's `confidence` is never read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Depth {
    /// The probability of each level, in [`DepthLevel::ALL`] order.
    distribution: [Probability; 3],
    expected: f64,
}

/// A level of [`Depth`], from the shallowest to the deepest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DepthLevel {
    /// Answerable by reading one line.
    OneLine,
    /// Needs reading the change.
    TheChange,
    /// Needs an understanding of how the change affects the code around it.
    CodeAround,
}

/// A depth Jev returned whose expected value is not a level's place: not a
/// number from 0 to 2. A reply holding one is invalid, never coerced.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("{0} is not an expected depth: it must be from 0 to 2")]
pub struct InvalidDepth(f64);

/// A candidate that passed its question check, with Jev's answers, its
/// depth, the policy that passed it, with its cut-offs, the criteria of its
/// rubric the show rule kept, and its hint if the hint passed its checks
/// too. Only questions are ever shown.
///
/// Its `Debug` output shows no question text.
#[derive(Clone, PartialEq)]
pub struct Question {
    candidate: Candidate,
    /// The criteria of the candidate's rubric the show rule kept, in order.
    rubric: Vec<Criterion>,
    /// The places, counting from 0, of the criteria it dropped, in order.
    dropped: Vec<usize>,
    answers: QuestionCheckAnswers,
    policy: AssessmentPolicy,
    hint: Option<Hint>,
}

/// A candidate the show rule withheld, with Jev's answers to its question
/// check, the policy that banded them, with its cut-offs, and the checks
/// that were not as the rule needs them. It is never shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Withheld {
    /// Boxed, as is the policy, so the show rule's `Err` stays small.
    candidate: Box<Candidate>,
    answers: QuestionCheckAnswers,
    policy: Box<AssessmentPolicy>,
    failed: Vec<CandidateCheck>,
}

impl Candidate {
    /// The yes/no questions its question check asks Jev, in order: checks
    /// 1 to 4; one per criterion; one per criterion again, whether it asks
    /// only for what the question needs; one option rating per option; one
    /// per rationale, then "why it matters"; and the hint's two.
    #[must_use]
    pub fn checks(&self) -> Vec<CandidateCheck> {
        let letters = match self.question_type() {
            QuestionType::SingleChoice => OptionLetter::ALL,
        };
        [
            CandidateCheck::Answerable,
            CandidateCheck::AboutArea,
            CandidateCheck::NeedsChange,
            CandidateCheck::NoGiveaway,
        ]
        .into_iter()
        .chain((0..self.rubric().len()).map(CandidateCheck::Criterion))
        .chain((0..self.rubric().len()).map(CandidateCheck::CriterionMinimum))
        .chain(letters.into_iter().map(CandidateCheck::Option))
        .chain(letters.into_iter().map(CandidateCheck::Rationale))
        .chain([
            CandidateCheck::WhyItMatters,
            CandidateCheck::HintPointsToEvidence,
            CandidateCheck::HintHidesAnswerKey,
        ])
        .collect()
    }
}

impl CandidateCheck {
    /// The job whose cut-offs band its answer.
    #[must_use]
    pub fn job(self) -> Job {
        match self {
            Self::Answerable
            | Self::AboutArea
            | Self::NeedsChange
            | Self::NoGiveaway
            | Self::Criterion(_)
            | Self::CriterionMinimum(_) => Job::QuestionCheck,
            Self::Option(_) => Job::OptionRating,
            Self::Rationale(_)
            | Self::WhyItMatters
            | Self::HintPointsToEvidence
            | Self::HintHidesAnswerKey => Job::RationaleAndHintChecks,
        }
    }

    /// Whether it checks the hint, which the show rule leaves aside.
    fn is_hint(self) -> bool {
        matches!(self, Self::HintPointsToEvidence | Self::HintHidesAnswerKey)
    }
}

impl QuestionCheckAnswers {
    /// Jev's yes/no `answers`, each to one check, and its `depth` score. A
    /// check answered twice keeps its last answer.
    #[must_use]
    pub fn new(
        answers: impl IntoIterator<Item = (CandidateCheck, Probability)>,
        depth: Depth,
    ) -> Self {
        Self {
            yes_no: answers.into_iter().collect(),
            depth,
        }
    }

    /// Jev's answer to `check`, if it was asked.
    #[must_use]
    pub fn get(&self, check: CandidateCheck) -> Option<Probability> {
        self.yes_no.get(&check).copied()
    }

    /// Every yes/no answer, by check, in [`CandidateCheck`] order.
    pub fn iter(&self) -> impl Iterator<Item = (CandidateCheck, Probability)> + '_ {
        self.yes_no.iter().map(|(&check, &answer)| (check, answer))
    }

    /// Jev's depth score.
    #[must_use]
    pub fn depth(&self) -> Depth {
        self.depth
    }
}

impl Depth {
    /// The depth with this `distribution` over the levels, in
    /// [`DepthLevel::ALL`] order, and this `expected` value: the
    /// probability-weighted mean of the levels' places, counting from 0, so
    /// from 0 to 2.
    pub fn new(distribution: [Probability; 3], expected: f64) -> Result<Self, InvalidDepth> {
        if (0.0..=2.0).contains(&expected) {
            Ok(Self {
                distribution,
                expected,
            })
        } else {
            Err(InvalidDepth(expected))
        }
    }

    /// The probability of `level`, as returned.
    #[must_use]
    pub fn probability(&self, level: DepthLevel) -> Probability {
        self.distribution[level.index()]
    }

    /// The expected value, as returned: from 0, answerable by reading one
    /// line, to 2, needing an understanding of the code around the change.
    #[must_use]
    pub fn expected(&self) -> f64 {
        self.expected
    }
}

impl DepthLevel {
    /// Every level, from the shallowest to the deepest.
    pub const ALL: [Self; 3] = [Self::OneLine, Self::TheChange, Self::CodeAround];

    /// Its place in [`DepthLevel::ALL`].
    fn index(self) -> usize {
        match self {
            Self::OneLine => 0,
            Self::TheChange => 1,
            Self::CodeAround => 2,
        }
    }
}

/// The show rule: `candidate` becomes a question only when Jev's `answers`
/// to its question check are what its question type needs, under `policy`.
/// Anything unclear fails it, and so does a check left unanswered, except
/// where a check below drops a criterion or the hint instead.
///
/// For a single-choice question: checks 1 to 4 and every criterion's check
/// are clear yes; every rationale check and the "why it matters" check are
/// clear yes; exactly one option rating is clear yes and it is the answer
/// key; and the other two are clear no.
///
/// A criterion whose [`CandidateCheck::CriterionMinimum`] is clear no, or
/// left unanswered, asks for more than the question needs, so it is
/// dropped, as a failing hint is: the question keeps the other criteria, in
/// order, and reasoning is assessed against those alone. The check drops
/// only what it is sure of, so a criterion it is unclear about stays, and
/// reasoning is assessed against it as usual. When every criterion is
/// dropped, the candidate is withheld, naming each criterion's minimum
/// check. The hint's checks never withhold a question. A hint that isn't
/// clear yes on both is dropped, and the question has no hint.
///
/// The withheld candidate keeps itself, Jev's answers and `policy`, and
/// names every check that failed, in [`Candidate::checks`] order.
pub fn show_rule(
    candidate: Candidate,
    answers: QuestionCheckAnswers,
    policy: &AssessmentPolicy,
) -> Result<Question, Withheld> {
    let passes = |check: CandidateCheck| {
        answers.get(check).is_some_and(|answer| {
            passing(&candidate, check).contains(&policy.band(check.job(), answer))
        })
    };
    let checks = candidate.checks();
    let (kept, dropped): (Vec<usize>, Vec<usize>) = (0..candidate.rubric().len())
        .partition(|&place| passes(CandidateCheck::CriterionMinimum(place)));
    let failed: Vec<CandidateCheck> = checks
        .iter()
        .copied()
        .filter(|check| !check.is_hint())
        .filter(|&check| match check {
            CandidateCheck::CriterionMinimum(_) => kept.is_empty(),
            _ => !passes(check),
        })
        .collect();
    let hint = checks
        .iter()
        .copied()
        .filter(|check| check.is_hint())
        .all(passes)
        .then(|| candidate.hint().clone());
    if failed.is_empty() {
        let rubric = kept
            .iter()
            .map(|&place| candidate.rubric()[place].clone())
            .collect();
        Ok(Question {
            candidate,
            rubric,
            dropped,
            answers,
            policy: policy.clone(),
            hint,
        })
    } else {
        Err(Withheld {
            candidate: Box::new(candidate),
            answers,
            policy: Box::new(policy.clone()),
            failed,
        })
    }
}

/// The band the show rule needs for `candidate`'s `check`.
fn needed(candidate: &Candidate, check: CandidateCheck) -> Band {
    match check {
        CandidateCheck::Option(letter) if letter != candidate.answer_key() => Band::ClearNo,
        _ => Band::ClearYes,
    }
}

/// The bands of Jev's answer to `candidate`'s `check` that pass it: the one
/// [`needed`], or, for a [`CandidateCheck::CriterionMinimum`], clear yes or
/// unclear, as the check drops only what it is sure of.
fn passing(candidate: &Candidate, check: CandidateCheck) -> &'static [Band] {
    match check {
        CandidateCheck::CriterionMinimum(_) => &[Band::ClearYes, Band::Unclear],
        _ => match needed(candidate, check) {
            Band::ClearYes => &[Band::ClearYes],
            Band::Unclear => &[Band::Unclear],
            Band::ClearNo => &[Band::ClearNo],
        },
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

    /// Why the option `letter` is right or wrong. Jev checked it against the
    /// evidence before the question was shown.
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

    /// What the question is getting at. Jev checked it against the evidence
    /// before the question was shown.
    #[must_use]
    pub fn why_it_matters(&self) -> &str {
        self.candidate.why_it_matters()
    }

    /// The criteria written reasoning is assessed against: those of the
    /// generator's 1 or 2 the show rule kept, in order, at least one. Jev
    /// checked that each follows from the evidence and the answer key, and
    /// can be judged from written reasoning, and didn't clearly say that it
    /// asks for more than the question needs.
    #[must_use]
    pub fn rubric(&self) -> &[Criterion] {
        &self.rubric
    }

    /// The criteria the show rule dropped from the generator's rubric,
    /// because Jev's answer to their [`CandidateCheck::CriterionMinimum`]
    /// was clear no, or missing, in order, each with its place in that
    /// rubric, counting from 0. The answer that dropped each is in
    /// [`answers`](Self::answers). Reasoning is never assessed against them.
    #[must_use = "it only reads the criteria"]
    pub fn dropped_criteria(&self) -> impl Iterator<Item = (usize, &Criterion)> + '_ {
        self.dropped
            .iter()
            .map(|&place| (place, &self.candidate.rubric()[place]))
    }

    /// Its hint, if the hint points to the evidence without revealing the
    /// answer key. Without one, the question shows "No hint for this
    /// question".
    #[must_use]
    pub fn hint(&self) -> Option<&Hint> {
        self.hint.as_ref()
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

    /// How much understanding of the change it needs, as Jev scored it. It
    /// orders questions, and never passed or failed this one.
    #[must_use]
    pub fn depth(&self) -> Depth {
        self.answers.depth
    }

    /// The policy that passed it: its version, and the cut-offs it banded
    /// the answers with. Its option ratings and the outcomes of its
    /// responses are banded with them too.
    #[must_use]
    pub fn policy(&self) -> &AssessmentPolicy {
        &self.policy
    }
}

/// Shows its candidate, without text, and Jev's answers.
impl fmt::Debug for Question {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Question")
            .field("candidate", &self.candidate)
            .field("rubric", &self.rubric)
            .field("dropped", &self.dropped)
            .field("answers", &self.answers)
            .field("policy", &self.policy)
            .field("hint", &self.hint)
            .finish()
    }
}

impl Withheld {
    /// What identifies the candidate withheld.
    #[must_use]
    pub fn id(&self) -> &CandidateId {
        self.candidate.id()
    }

    /// The candidate withheld.
    #[must_use]
    pub fn candidate(&self) -> &Candidate {
        &self.candidate
    }

    /// Jev's answers to its question check, as returned.
    #[must_use]
    pub fn answers(&self) -> &QuestionCheckAnswers {
        &self.answers
    }

    /// The policy that withheld it: its version, and the cut-offs it banded
    /// the answers with.
    #[must_use]
    pub fn policy(&self) -> &AssessmentPolicy {
        &self.policy
    }

    /// The band of Jev's answer to `check` under the policy that withheld
    /// it, or none when it went unanswered.
    #[must_use]
    pub fn band(&self, check: CandidateCheck) -> Option<Band> {
        self.answers
            .get(check)
            .map(|answer| self.policy.band(check.job(), answer))
    }

    /// The checks that were not as the show rule needs them, in
    /// [`Candidate::checks`] order.
    #[must_use]
    pub fn failed(&self) -> &[CandidateCheck] {
        &self.failed
    }

    /// The band the show rule needed for its `check`: clear no for an
    /// option rating that isn't the answer key's, and clear yes otherwise.
    /// For a [`CandidateCheck::CriterionMinimum`], clear yes is the side
    /// that keeps its criterion, though [`passing`](Self::passing) holds
    /// every band that keeps it.
    #[must_use]
    pub fn needed(&self, check: CandidateCheck) -> Band {
        needed(&self.candidate, check)
    }

    /// The bands of Jev's answer to its `check` that would have passed it:
    /// the one it [`needed`](Self::needed), or, for a
    /// [`CandidateCheck::CriterionMinimum`], clear yes or unclear, either of
    /// which keeps its criterion.
    #[must_use]
    pub fn passing(&self, check: CandidateCheck) -> &'static [Band] {
        passing(&self.candidate, check)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::assessment_policy::CutOffs;
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

    /// Answers to every check of `candidate` that pass it: 0.9 for each,
    /// except 0.1 for the options that aren't its answer key.
    pub(crate) fn passing(candidate: &Candidate) -> Vec<(CandidateCheck, Probability)> {
        candidate
            .checks()
            .into_iter()
            .map(|check| {
                let answer = match check {
                    CandidateCheck::Option(letter) if letter != candidate.answer_key() => 0.1,
                    _ => 0.9,
                };
                (check, probability(answer))
            })
            .collect()
    }

    /// A depth whose expected value is `expected`.
    pub(crate) fn depth(expected: f64) -> Depth {
        Depth::new([0.2, 0.3, 0.5].map(probability), expected).expect("from 0 to 2")
    }

    /// Answers rating options A, B and C at `a`, `b` and `c`.
    fn options(a: f64, b: f64, c: f64) -> Vec<(CandidateCheck, Probability)> {
        OptionLetter::ALL
            .into_iter()
            .map(CandidateCheck::Option)
            .zip([a, b, c].map(probability))
            .collect()
    }

    /// The show rule's verdict on a candidate whose answer key is
    /// `answer_key`, answered to pass except for `answers`.
    fn show(
        answer_key: OptionLetter,
        answers: Vec<(CandidateCheck, Probability)>,
    ) -> Result<Question, Withheld> {
        let candidate = candidate("domain", 0, answer_key);
        let mut all = passing(&candidate);
        all.extend(answers);
        show_rule(
            candidate,
            QuestionCheckAnswers::new(all, depth(1.3)),
            AssessmentPolicy::current(),
        )
    }

    fn failed(result: Result<Question, Withheld>) -> Vec<CandidateCheck> {
        result.expect_err("withheld").failed().to_vec()
    }

    #[test]
    fn the_question_check_asks_every_check_in_order() {
        use CandidateCheck::*;

        assert_eq!(
            candidate("domain", 0, OptionLetter::A).checks(),
            [
                Answerable,
                AboutArea,
                NeedsChange,
                NoGiveaway,
                Criterion(0),
                Criterion(1),
                CriterionMinimum(0),
                CriterionMinimum(1),
                Option(OptionLetter::A),
                Option(OptionLetter::B),
                Option(OptionLetter::C),
                Rationale(OptionLetter::A),
                Rationale(OptionLetter::B),
                Rationale(OptionLetter::C),
                WhyItMatters,
                HintPointsToEvidence,
                HintHidesAnswerKey,
            ]
        );
    }

    #[test]
    fn each_check_is_banded_by_its_jobs_cut_offs() {
        use CandidateCheck::*;

        for (check, job) in [
            (Answerable, Job::QuestionCheck),
            (AboutArea, Job::QuestionCheck),
            (NeedsChange, Job::QuestionCheck),
            (NoGiveaway, Job::QuestionCheck),
            (Criterion(2), Job::QuestionCheck),
            (CriterionMinimum(1), Job::QuestionCheck),
            (Option(OptionLetter::B), Job::OptionRating),
            (Rationale(OptionLetter::C), Job::RationaleAndHintChecks),
            (WhyItMatters, Job::RationaleAndHintChecks),
            (HintPointsToEvidence, Job::RationaleAndHintChecks),
            (HintHidesAnswerKey, Job::RationaleAndHintChecks),
        ] {
            assert_eq!(check.job(), job, "{check:?}");
        }
    }

    #[test]
    fn a_candidate_that_passes_every_check_is_shown_with_its_hint() {
        let candidate = candidate("domain", 0, OptionLetter::B);
        let answers = passing(&candidate);

        let question = show_rule(
            candidate,
            QuestionCheckAnswers::new(answers.clone(), depth(1.3)),
            AssessmentPolicy::current(),
        )
        .expect("shown");

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
        assert_eq!(
            question.hint().map(Hint::text),
            Some("Look at how the key is made.")
        );
        assert_eq!(question.policy(), AssessmentPolicy::current());
        assert_eq!(
            question
                .answers()
                .get(CandidateCheck::Option(OptionLetter::B)),
            Some(probability(0.9))
        );
        assert_eq!(question.answers().iter().collect::<Vec<_>>(), answers);
        assert_eq!(question.depth(), depth(1.3));
    }

    #[test]
    fn a_check_the_learner_s_cut_offs_make_clear_yes_passes_and_the_question_keeps_them() {
        let candidate = candidate("domain", 0, OptionLetter::A);
        let mut answers = passing(&candidate);
        answers.push((CandidateCheck::NeedsChange, probability(0.6)));
        let answers = QuestionCheckAnswers::new(answers, depth(1.0));
        let lower_bar = AssessmentPolicy::with_cut_offs([(
            Job::QuestionCheck,
            CutOffs::new(0.55, 0.20).expect("valid"),
        )]);

        let withheld = show_rule(
            candidate.clone(),
            answers.clone(),
            AssessmentPolicy::current(),
        );
        let shown = show_rule(candidate, answers, &lower_bar);

        assert_eq!(failed(withheld), [CandidateCheck::NeedsChange]);
        let question = shown.expect("clear yes under the learner's cut-offs");
        assert_eq!(question.policy(), &lower_bar);
    }

    #[test]
    fn depth_is_kept_as_returned() {
        let distribution = [0.1, 0.25, 0.6].map(probability);

        let depth = Depth::new(distribution, 1.5).expect("from 0 to 2");

        assert_eq!(depth.probability(DepthLevel::OneLine), probability(0.1));
        assert_eq!(depth.probability(DepthLevel::TheChange), probability(0.25));
        assert_eq!(depth.probability(DepthLevel::CodeAround), probability(0.6));
        assert_eq!(depth.expected().to_bits(), 1.5_f64.to_bits());
    }

    #[test]
    fn an_expected_depth_is_from_0_to_2() {
        let distribution = [0.0, 0.0, 1.0].map(probability);
        for expected in [0.0, 2.0] {
            assert!(Depth::new(distribution, expected).is_ok(), "{expected}");
        }
        for expected in [-0.01, 2.01, f64::NAN, f64::INFINITY] {
            assert!(Depth::new(distribution, expected).is_err(), "{expected}");
        }
    }

    #[test]
    fn depth_never_passes_or_fails_a_candidate() {
        for expected in [0.0, 1.0, 2.0] {
            let candidate = candidate("domain", 0, OptionLetter::A);
            let answers = passing(&candidate);

            let question = show_rule(
                candidate,
                QuestionCheckAnswers::new(answers, depth(expected)),
                AssessmentPolicy::current(),
            );

            assert!(question.is_ok(), "{expected}");
        }
        let deep_but_unclear = show_rule(
            candidate("domain", 0, OptionLetter::A),
            QuestionCheckAnswers::new(options(0.5, 0.1, 0.1), depth(2.0)),
            AssessmentPolicy::current(),
        );
        assert!(deep_but_unclear.is_err());
    }

    #[test]
    fn the_answer_key_clearly_supported_and_the_others_clearly_not_is_shown() {
        for answers in [options(0.2, 0.8, 0.0), options(0.0, 1.0, 0.2)] {
            assert!(show(OptionLetter::B, answers).is_ok());
        }
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
    fn each_check_not_clear_yes_withholds_the_candidate() {
        use CandidateCheck::*;

        for check in [
            Answerable,
            AboutArea,
            NeedsChange,
            NoGiveaway,
            Criterion(0),
            Criterion(1),
            Rationale(OptionLetter::A),
            Rationale(OptionLetter::B),
            Rationale(OptionLetter::C),
            WhyItMatters,
        ] {
            for answer in [0.79, 0.5, 0.2, 0.0] {
                assert_eq!(
                    failed(show(OptionLetter::A, vec![(check, probability(answer))])),
                    [check],
                    "{check:?} at {answer}"
                );
            }
        }
    }

    #[test]
    fn a_failed_rationale_withholds_the_candidate() {
        let rationale = CandidateCheck::Rationale(OptionLetter::B);

        assert_eq!(
            failed(show(OptionLetter::A, vec![(rationale, probability(0.6))])),
            [rationale]
        );
    }

    #[test]
    fn every_failed_check_is_named_in_order() {
        use CandidateCheck::*;

        assert_eq!(
            failed(show(
                OptionLetter::A,
                vec![
                    (WhyItMatters, probability(0.1)),
                    (Answerable, probability(0.5)),
                    (Option(OptionLetter::C), probability(0.9)),
                ]
            )),
            [Answerable, Option(OptionLetter::C), WhyItMatters]
        );
    }

    #[test]
    fn a_check_left_unanswered_withholds_the_candidate() {
        let candidate = candidate("domain", 0, OptionLetter::A);
        for missing in candidate.checks() {
            let answers = passing(&candidate)
                .into_iter()
                .filter(|&(check, _)| check != missing);

            let shown = show_rule(
                candidate.clone(),
                QuestionCheckAnswers::new(answers, depth(1.3)),
                AssessmentPolicy::current(),
            );

            match missing {
                CandidateCheck::HintPointsToEvidence | CandidateCheck::HintHidesAnswerKey => {
                    assert_eq!(shown.expect("shown").hint(), None);
                }
                CandidateCheck::CriterionMinimum(place) => {
                    let question = shown.expect("shown, with the other criterion");
                    assert_eq!(
                        question
                            .dropped_criteria()
                            .map(|(dropped, _)| dropped)
                            .collect::<Vec<_>>(),
                        [place]
                    );
                }
                _ => assert_eq!(failed(shown), [missing]),
            }
        }
    }

    #[test]
    fn a_hint_not_clear_yes_on_both_checks_is_dropped_and_its_question_kept() {
        for (points, hides) in [(0.5, 0.9), (0.9, 0.79), (0.1, 0.9), (0.9, 0.0), (0.2, 0.2)] {
            let question = show(
                OptionLetter::A,
                vec![
                    (CandidateCheck::HintPointsToEvidence, probability(points)),
                    (CandidateCheck::HintHidesAnswerKey, probability(hides)),
                ],
            )
            .expect("a failed hint keeps its question");

            assert_eq!(question.hint(), None, "{points}, {hides}");
        }
    }

    /// The statements of `criteria`, in order.
    fn statements<'a>(criteria: impl IntoIterator<Item = &'a Criterion>) -> Vec<&'a str> {
        criteria.into_iter().map(Criterion::statement).collect()
    }

    /// The show rule's verdict on a candidate answered to pass, but with
    /// `answer` to whether its first criterion asks only for what the
    /// question needs, or no answer to it.
    fn show_with_first_minimum(answer: Option<f64>) -> Result<Question, Withheld> {
        let minimum = CandidateCheck::CriterionMinimum(0);
        let candidate = candidate("domain", 0, OptionLetter::A);
        let answers = passing(&candidate)
            .into_iter()
            .filter(|&(check, _)| check != minimum)
            .chain(answer.map(|answer| (minimum, probability(answer))));
        show_rule(
            candidate,
            QuestionCheckAnswers::new(answers, depth(1.3)),
            AssessmentPolicy::current(),
        )
    }

    #[test]
    fn a_criterion_whose_minimum_check_is_clear_no_is_dropped_and_the_question_shown() {
        let minimum = CandidateCheck::CriterionMinimum(0);
        // Clear no from 0.20 down, and no answer, which fails any check.
        for answer in [Some(0.20), Some(0.0), None] {
            let question = show_with_first_minimum(answer).expect("shown, with the criterion left");

            assert_eq!(
                statements(question.rubric()),
                ["Says the provider charges once"],
                "{answer:?}"
            );
            let dropped: Vec<(usize, &str)> = question
                .dropped_criteria()
                .map(|(place, criterion)| (place, criterion.statement()))
                .collect();
            assert_eq!(dropped, [(0, "Links the key to deduplication")]);
            assert_eq!(
                question.answers().get(minimum),
                answer.map(probability),
                "the answer that dropped it is kept"
            );
        }
    }

    #[test]
    fn a_criterion_whose_minimum_check_is_unclear_or_clear_yes_stays_in_the_rubric() {
        // Unclear from 0.21 to 0.79, and clear yes from 0.80.
        for answer in [0.21, 0.5, 0.79, 0.80, 1.0] {
            let question = show_with_first_minimum(Some(answer)).expect("shown");

            assert_eq!(
                statements(question.rubric()),
                [
                    "Links the key to deduplication",
                    "Says the provider charges once"
                ],
                "{answer}"
            );
            assert_eq!(question.dropped_criteria().count(), 0, "{answer}");
        }
    }

    #[test]
    fn a_question_keeps_its_criteria_in_order_when_every_one_asks_only_what_it_needs() {
        let question = show(OptionLetter::A, Vec::new()).expect("shown");

        assert_eq!(
            statements(question.rubric()),
            [
                "Links the key to deduplication",
                "Says the provider charges once"
            ]
        );
        assert_eq!(question.dropped_criteria().count(), 0);
    }

    #[test]
    fn a_candidate_whose_every_criterion_is_a_clear_no_is_withheld_naming_each() {
        use CandidateCheck::*;

        let withheld = show(
            OptionLetter::A,
            vec![
                (CriterionMinimum(0), probability(0.20)),
                (CriterionMinimum(1), probability(0.0)),
            ],
        )
        .expect_err("no criterion is left");

        assert_eq!(
            withheld.failed(),
            [CriterionMinimum(0), CriterionMinimum(1)]
        );
        assert_eq!(withheld.band(CriterionMinimum(0)), Some(Band::ClearNo));
        assert_eq!(withheld.needed(CriterionMinimum(1)), Band::ClearYes);
        assert_eq!(
            withheld.passing(CriterionMinimum(1)),
            [Band::ClearYes, Band::Unclear]
        );
        assert_eq!(
            withheld.passing(Option(OptionLetter::B)),
            [Band::ClearNo],
            "an option besides the answer key"
        );
        assert_eq!(withheld.passing(Criterion(0)), [Band::ClearYes]);
    }

    #[test]
    fn a_clear_no_and_an_unclear_criterion_keep_the_question_with_the_unclear_one() {
        use CandidateCheck::*;

        let question = show(
            OptionLetter::A,
            vec![
                (CriterionMinimum(0), probability(0.1)),
                (CriterionMinimum(1), probability(0.5)),
            ],
        )
        .expect("shown, with the unclear criterion");

        assert_eq!(
            statements(question.rubric()),
            ["Says the provider charges once"]
        );
        assert_eq!(
            question
                .dropped_criteria()
                .map(|(place, _)| place)
                .collect::<Vec<_>>(),
            [0]
        );
    }

    #[test]
    fn a_criterion_check_not_clear_yes_still_withholds_whether_or_not_its_criterion_is_dropped() {
        use CandidateCheck::*;

        // Kept as clear yes, kept as unclear, and dropped as clear no.
        for minimum in [0.9, 0.5, 0.1] {
            assert_eq!(
                failed(show(
                    OptionLetter::A,
                    vec![
                        (Criterion(0), probability(0.5)),
                        (CriterionMinimum(0), probability(minimum)),
                    ],
                )),
                [Criterion(0)],
                "minimum check at {minimum}"
            );
        }
    }

    #[test]
    fn a_withheld_candidate_names_itself() {
        let withheld = show_rule(
            candidate("observability", 2, OptionLetter::A),
            QuestionCheckAnswers::new([], depth(1.3)),
            AssessmentPolicy::current(),
        )
        .expect_err("withheld");

        assert_eq!(withheld.id().area(), &area("observability"));
        assert_eq!(withheld.id().written(), 2);
    }

    #[test]
    fn a_withheld_candidate_keeps_itself_jev_s_answers_and_the_policy_that_banded_them() {
        let candidate = candidate("domain", 0, OptionLetter::C);
        let mut answers = passing(&candidate);
        answers.push((CandidateCheck::NoGiveaway, probability(0.82)));
        let answers = QuestionCheckAnswers::new(answers, depth(1.3));
        let higher_bar = AssessmentPolicy::with_cut_offs([(
            Job::QuestionCheck,
            CutOffs::new(0.85, 0.20).expect("valid"),
        )]);

        let withheld =
            show_rule(candidate.clone(), answers.clone(), &higher_bar).expect_err("withheld");

        assert_eq!(withheld.candidate(), &candidate);
        assert_eq!(withheld.candidate().answer_key(), OptionLetter::C);
        assert_eq!(withheld.answers(), &answers);
        assert_eq!(withheld.policy(), &higher_bar);
        assert_eq!(withheld.failed(), [CandidateCheck::NoGiveaway]);
        assert_eq!(withheld.needed(CandidateCheck::NoGiveaway), Band::ClearYes);
        assert_eq!(
            withheld.needed(CandidateCheck::Option(OptionLetter::C)),
            Band::ClearYes,
            "its answer key"
        );
        assert_eq!(
            withheld.needed(CandidateCheck::Option(OptionLetter::A)),
            Band::ClearNo
        );
    }

    #[test]
    fn a_withheld_candidate_bands_each_answer_with_the_policy_that_withheld_it() {
        let candidate = candidate("domain", 0, OptionLetter::A);
        let mut answers = passing(&candidate);
        answers.push((CandidateCheck::NoGiveaway, probability(0.82)));
        let higher_bar = AssessmentPolicy::with_cut_offs([(
            Job::QuestionCheck,
            CutOffs::new(0.85, 0.20).expect("valid"),
        )]);

        let withheld = show_rule(
            candidate,
            QuestionCheckAnswers::new(answers, depth(1.3)),
            &higher_bar,
        )
        .expect_err("withheld");

        assert_eq!(
            withheld.band(CandidateCheck::NoGiveaway),
            Some(Band::Unclear)
        );
        assert_eq!(
            withheld.band(CandidateCheck::Option(OptionLetter::B)),
            Some(Band::ClearNo)
        );
        assert_eq!(withheld.band(CandidateCheck::Criterion(2)), None);
    }

    #[test]
    fn a_withheld_candidate_s_debug_output_never_shows_its_text() {
        let withheld = show(
            OptionLetter::A,
            vec![(CandidateCheck::NoGiveaway, probability(0.46))],
        )
        .expect_err("withheld");
        let debug = format!("{withheld:?}");

        for text in [
            "retry reuse",
            "Option A",
            "Rationale A",
            "charge twice",
            "Look at",
        ] {
            assert!(!debug.contains(text), "{debug}");
        }
    }

    #[test]
    fn debug_output_never_shows_a_questions_text() {
        let question = show(OptionLetter::A, Vec::new()).expect("shown");
        let debug = format!("{question:?}");

        for text in [
            "retry reuse",
            "Option A",
            "Rationale A",
            "charge twice",
            "Look at",
        ] {
            assert!(!debug.contains(text), "{debug}");
        }
    }
}
