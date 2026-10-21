//! Why Practice shows fewer questions than were asked for, or none, in
//! words: from the candidates the generator wrote last, why the plain
//! checks dropped each one they dropped, and each candidate the question
//! check withheld, with Jev's answers and the policy that banded them,
//! which Practice keeps. No questions passed checks gives these reasons,
//! and so does the shortfall's See why fewer passed.
//!
//! Each check that withholds a candidate is worded here alone, with one
//! sentence for each way Jev can answer it against the show rule: "Jev
//! said …" for a clear answer, "Jev couldn't tell whether …" for an
//! unclear one. The hint's checks never withhold a candidate, so they are
//! never a reason. The reasons say whose cut-offs decided, policy 0's or
//! the learner's own from config, and never a probability or a cut-off:
//! Settings and Inspect hold the numbers.

use std::collections::{BTreeMap, BTreeSet};

use whetstone_core::{
    Band, CandidateCheck, CutOffsOrigin, DropReason, OptionLetter, Part, Withheld,
};

/// The reasons, a line each, as the learner reads them, for the candidates
/// the generator wrote last: `passed` of them passed their checks, the
/// plain checks dropped one for each of `dropped`, and the show rule
/// withheld `withheld`, each banded with its own policy.
///
/// First comes the count written and withheld, and whose cut-offs decided;
/// then each reason a candidate was withheld, with how many it withheld,
/// most first, then in the order the checks are asked; then the plain
/// checks' drops, counted apart.
pub(crate) fn reasons(passed: usize, dropped: &[DropReason], withheld: &[Withheld]) -> Vec<String> {
    let mut lines = vec![count(passed + dropped.len() + withheld.len(), withheld)];
    let mut counts = BTreeMap::new();
    for reason in withheld.iter().flat_map(each_reason) {
        *counts.entry(reason).or_insert(0) += 1;
    }
    let all = withheld.len();
    for (reason, count) in most_first(counts) {
        let lead = if count == all && count > 1 {
            format!("all {count}")
        } else {
            count.to_string()
        };
        lines.push(format!("{lead}: {}", reason.sentence()));
    }
    for (reason, count) in most_first(dropped.iter().map(|&reason| (reason, 1))) {
        let subject = if count == 1 { "it" } else { "each" };
        lines.push(format!(
            "{count} dropped: {}",
            words_dropped(reason, subject)
        ));
    }
    lines
}

/// How many were `written` and `withheld`, and whose cut-offs withheld
/// them.
fn count(written: usize, withheld: &[Withheld]) -> String {
    let Some(first) = withheld.first() else {
        return format!("{written} written");
    };
    let (mut defaults, mut learners) = (false, false);
    for withheld in withheld {
        for check in withheld.failed() {
            match withheld.policy().cut_offs(check.job()).origin() {
                CutOffsOrigin::Default => defaults = true,
                CutOffsOrigin::Learner => learners = true,
            }
        }
    }
    let version = first.policy().version();
    let whose = match (learners, defaults) {
        (false, _) => format!("policy {version}'s cut-offs"),
        (true, false) => "your cut-offs from config".to_owned(),
        (true, true) => format!("your cut-offs from config and policy {version}'s"),
    };
    format!(
        "{written} written · {} withheld, using {whose}",
        withheld.len()
    )
}

/// Each reason `withheld` was withheld, once, in order.
fn each_reason(withheld: &Withheld) -> BTreeSet<Reason> {
    let answer_key = withheld.candidate().answer_key();
    withheld
        .failed()
        .iter()
        .filter_map(|&check| {
            let band = withheld
                .answers()
                .get(check)
                .map(|answer| withheld.policy().band(check.job(), answer));
            Some(Reason {
                asks: Asks::of(check, answer_key)?,
                answered: Answered::of(band),
            })
        })
        .collect()
}

/// One reason a candidate was withheld: what a check it failed asks, and
/// how Jev answered it. Reasons order as the checks are asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Reason {
    asks: Asks,
    answered: Answered,
}

/// What a check that withholds a candidate asks, as the reasons group the
/// checks: every criterion's check together, every rationale's together,
/// and the answer key's option rating apart from the other options'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Asks {
    Answerable,
    AboutArea,
    NeedsChange,
    NoGiveaway,
    Criterion,
    AnswerKey,
    OtherOption,
    Rationale,
    WhyItMatters,
}

/// How Jev answered a check that withheld a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Answered {
    /// Clearly, against what the show rule needs: no, or, for an option
    /// besides the answer key, yes.
    Clearly,
    /// Unclearly: Jev couldn't tell.
    Unclearly,
    /// Not at all.
    Not,
}

impl Reason {
    /// The plain sentence that gives it.
    fn sentence(self) -> String {
        let (whether, said) = self.asks.words();
        match self.answered {
            Answered::Clearly => format!("Jev said {said}"),
            Answered::Unclearly => format!("Jev couldn't tell whether {whether}"),
            Answered::Not => format!("Jev didn't answer whether {whether}"),
        }
    }
}

impl Asks {
    /// What `check`, of a candidate whose answer key is `answer_key`, asks,
    /// or none for the hint's checks, which never withhold a candidate.
    fn of(check: CandidateCheck, answer_key: OptionLetter) -> Option<Self> {
        Some(match check {
            CandidateCheck::Answerable => Self::Answerable,
            CandidateCheck::AboutArea => Self::AboutArea,
            CandidateCheck::NeedsChange => Self::NeedsChange,
            CandidateCheck::NoGiveaway => Self::NoGiveaway,
            CandidateCheck::Criterion(_) => Self::Criterion,
            CandidateCheck::Option(letter) if letter == answer_key => Self::AnswerKey,
            CandidateCheck::Option(_) => Self::OtherOption,
            CandidateCheck::Rationale(_) => Self::Rationale,
            CandidateCheck::WhyItMatters => Self::WhyItMatters,
            CandidateCheck::HintPointsToEvidence | CandidateCheck::HintHidesAnswerKey => {
                return None;
            }
        })
    }

    /// Its words: what it asks, after "whether", and what Jev said, after
    /// "Jev said", when it answered clearly against the show rule.
    fn words(self) -> (&'static str, &'static str) {
        match self {
            Self::Answerable => (
                "the question can be answered from the evidence",
                "the question can't be answered from the evidence",
            ),
            Self::AboutArea => (
                "the question is about its area",
                "the question isn't about its area",
            ),
            Self::NeedsChange => (
                "the question needs this change",
                "the question doesn't need this change",
            ),
            Self::NoGiveaway => (
                "the options give the answer away",
                "the options give the answer away",
            ),
            Self::Criterion => (
                "a rubric criterion follows from the evidence and can be judged from written \
                 reasoning",
                "a rubric criterion doesn't follow from the evidence, or can't be judged from \
                 written reasoning",
            ),
            Self::AnswerKey => (
                "the answer key answers the question",
                "the answer key doesn't answer the question",
            ),
            Self::OtherOption => (
                "an option besides the answer key answers the question",
                "an option besides the answer key answers the question",
            ),
            Self::Rationale => (
                "an option's rationale is supported by the evidence",
                "an option's rationale isn't supported by the evidence",
            ),
            Self::WhyItMatters => (
                "\"why it matters\" is supported by the evidence",
                "\"why it matters\" isn't supported by the evidence",
            ),
        }
    }
}

impl Answered {
    /// How Jev answered a check that withheld a candidate, from the `band`
    /// of its answer, or none when it went unanswered. A check withholds a
    /// candidate only when its band isn't the one the show rule needs, so a
    /// clear band is clearly against it.
    fn of(band: Option<Band>) -> Self {
        match band {
            Some(Band::ClearYes | Band::ClearNo) => Self::Clearly,
            Some(Band::Unclear) => Self::Unclearly,
            None => Self::Not,
        }
    }
}

/// The plain checks' `reason` for dropping candidates, after `subject`:
/// "it" for one, "each" for several.
fn words_dropped(reason: DropReason, subject: &str) -> String {
    match reason {
        DropReason::Options => format!("{subject} had options other than exactly A, B and C"),
        DropReason::AnswerKey => {
            format!("{subject} had an answer key that isn't one of its options")
        }
        DropReason::RubricSize => format!("{subject} had a rubric without 1 to 3 criteria"),
        DropReason::Citation => format!("{subject} cited lines that aren't in the evidence"),
        DropReason::Missing(part) => format!("{subject} was missing {}", missing(part)),
    }
}

/// A part a candidate was missing, after "it was missing".
fn missing(part: Part) -> String {
    match part {
        Part::Stem => "its stem".to_owned(),
        Part::Option(letter) => format!("option {letter}"),
        Part::Rationale(letter) => format!("the rationale for option {letter}"),
        Part::WhyItMatters => "its \"why it matters\" line".to_owned(),
        Part::Criterion => "part of a criterion".to_owned(),
        Part::Hint => "its hint".to_owned(),
        Part::Citations => "a citation".to_owned(),
    }
}

/// Each of `counted`'s reasons once, with its counts added up, the most
/// counted first, and otherwise in the order they come.
fn most_first<T: PartialEq>(counted: impl IntoIterator<Item = (T, usize)>) -> Vec<(T, usize)> {
    let mut totals: Vec<(T, usize)> = Vec::new();
    for (reason, count) in counted {
        match totals.iter_mut().find(|(seen, _)| *seen == reason) {
            Some((_, total)) => *total += count,
            None => totals.push((reason, count)),
        }
    }
    totals.sort_by(|(_, a), (_, b)| b.cmp(a));
    totals
}
