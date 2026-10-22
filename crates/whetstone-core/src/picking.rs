//! Picking: which of the questions that passed their checks a Practice
//! shows, and in what order, up to the learner's `questions` setting.

use std::collections::VecDeque;
use std::fmt;

use crate::area::AreaId;
use crate::question_check::Question;

/// How many questions a Practice asks for, as the `questions` setting in the
/// global config sets it: a whole number from 3 to 5, by default 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuestionCount(u8);

/// A `questions` value is not a whole number from 3 to 5. A config reader
/// returns it for a value that is not an integer, too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("`questions` must be a whole number from 3 to 5")]
pub struct QuestionCountError;

/// The questions picked to show, or why there are fewer than asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Picked {
    /// As many as were asked for, in the order to show them.
    All(Vec<Question>),
    /// Fewer passed their checks than were asked for: these, in the order
    /// to show them. It reads "*n* passed checks, *N* asked for".
    Shortfall {
        /// Every question that passed, in the order to show them.
        questions: Vec<Question>,
        /// How many were asked for.
        asked_for: QuestionCount,
    },
    /// No question passed its checks.
    NonePassed,
}

impl QuestionCount {
    /// The count when no config sets one.
    pub const DEFAULT: Self = Self(4);
    /// The fewest a config may set.
    const MIN: u8 = 3;
    /// The most a config may set.
    const MAX: u8 = 5;

    /// The count as a number.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Display for QuestionCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<i64> for QuestionCount {
    type Error = QuestionCountError;

    fn try_from(count: i64) -> Result<Self, Self::Error> {
        u8::try_from(count)
            .ok()
            .filter(|count| (Self::MIN..=Self::MAX).contains(count))
            .map(Self)
            .ok_or(QuestionCountError)
    }
}

/// Picks up to `count` of the `questions` that passed their checks, in the
/// order to show them, from the areas in `areas`, in the order the learner
/// chose them.
///
/// Each area's questions go deepest first, by their depth's expected value;
/// equally deep ones go in the order the generator wrote them. Picking takes
/// them in turn across the areas, round after round, one from each area that
/// has any left, until `count` are picked. Each round takes from the areas in
/// the order of their deepest remaining question; areas whose deepest are
/// equally deep go in area order. Depth only orders questions: it never
/// leaves one out while fewer than `count` are picked.
///
/// The questions may arrive in any order. Questions about an area not in
/// `areas` rank after the others when depths tie, by area ID.
#[must_use]
pub fn pick(questions: Vec<Question>, areas: &[AreaId], count: QuestionCount) -> Picked {
    let wanted = usize::from(count.get());
    let mut by_area = by_area(questions, areas);
    let mut picked = Vec::with_capacity(wanted);
    while picked.len() < wanted && by_area.iter().any(|of_area| !of_area.is_empty()) {
        let deepest = |of_area: &VecDeque<Question>| {
            of_area
                .front()
                .map_or(f64::NEG_INFINITY, |question| question.depth().expected())
        };
        // A stable sort of areas in area order leaves equally deep ones in
        // area order.
        let mut round: Vec<usize> = (0..by_area.len()).collect();
        round.sort_by(|&a, &b| deepest(&by_area[b]).total_cmp(&deepest(&by_area[a])));
        for area in round {
            if picked.len() == wanted {
                break;
            }
            picked.extend(by_area[area].pop_front());
        }
    }
    match picked.len() {
        0 => Picked::NonePassed,
        n if n == wanted => Picked::All(picked),
        _ => Picked::Shortfall {
            questions: picked,
            asked_for: count,
        },
    }
}

/// `questions` grouped by area, in area order, each area's deepest first,
/// then in the order written: `areas` first, in their order, then any other
/// by area ID.
fn by_area(mut questions: Vec<Question>, areas: &[AreaId]) -> Vec<VecDeque<Question>> {
    let rank = |question: &Question| {
        let area = question.area();
        let place = areas
            .iter()
            .position(|id| id == area)
            .unwrap_or(areas.len());
        (place, area.clone())
    };
    questions.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then(b.depth().expected().total_cmp(&a.depth().expected()))
            .then(a.id().written().cmp(&b.id().written()))
    });
    let mut by_area: Vec<VecDeque<Question>> = Vec::new();
    for question in questions {
        match by_area.last_mut() {
            Some(of_area)
                if of_area
                    .front()
                    .is_some_and(|first| first.area() == question.area()) =>
            {
                of_area.push_back(question);
            }
            _ => by_area.push(VecDeque::from([question])),
        }
    }
    by_area
}

impl Picked {
    /// The questions picked, in the order to show them: none when none
    /// passed.
    #[must_use]
    pub fn questions(&self) -> &[Question] {
        match self {
            Self::All(questions) | Self::Shortfall { questions, .. } => questions,
            Self::NonePassed => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assessment_policy::AssessmentPolicy;
    use crate::question::OptionLetter;
    use crate::question::tests::area;
    use crate::question_check::tests::{candidate, depth, passing, passing_probe};
    use crate::question_check::{QuestionCheckAnswers, show_rule};

    fn count(count: i64) -> QuestionCount {
        QuestionCount::try_from(count).expect("from 3 to 5")
    }

    /// A question about `area_id`, written in place `written_at`, whose
    /// depth's expected value is `deep`.
    fn question(area_id: &str, written_at: usize, deep: f64) -> Question {
        let candidate = candidate(area_id, written_at, OptionLetter::A);
        let answers = QuestionCheckAnswers::new(passing(&candidate), depth(deep))
            .with_probe(passing_probe(&candidate));
        show_rule(candidate, answers, AssessmentPolicy::current()).expect("shown")
    }

    fn areas(ids: &[&str]) -> Vec<AreaId> {
        ids.iter().map(|id| area(id)).collect()
    }

    /// Each picked question's area and place written.
    fn order(picked: &Picked) -> Vec<(String, usize)> {
        picked
            .questions()
            .iter()
            .map(|question| (question.area().as_str().to_owned(), question.id().written()))
            .collect()
    }

    fn expected(order: &[(&str, usize)]) -> Vec<(String, usize)> {
        order
            .iter()
            .map(|&(area, written)| (area.to_owned(), written))
            .collect()
    }

    #[test]
    fn questions_is_a_whole_number_from_3_to_5_by_default_4() {
        assert_eq!(QuestionCount::DEFAULT.get(), 4);
        for value in [3, 4, 5] {
            assert_eq!(count(value).get().to_string(), value.to_string());
            assert_eq!(count(value).to_string(), value.to_string());
        }
        for value in [2, 6, 0, -1, 256 + 4, i64::MIN, i64::MAX] {
            assert_eq!(
                QuestionCount::try_from(value),
                Err(QuestionCountError),
                "{value}"
            );
        }
    }

    #[test]
    fn an_areas_deepest_questions_come_first() {
        let picked = pick(
            vec![
                question("domain", 0, 0.4),
                question("domain", 1, 1.9),
                question("domain", 2, 1.0),
                question("domain", 3, 0.1),
            ],
            &areas(&["domain"]),
            count(3),
        );

        assert!(matches!(picked, Picked::All(_)));
        assert_eq!(
            order(&picked),
            expected(&[("domain", 1), ("domain", 2), ("domain", 0)])
        );
    }

    #[test]
    fn equally_deep_questions_go_in_the_order_written() {
        let picked = pick(
            vec![
                question("domain", 2, 1.0),
                question("domain", 0, 1.0),
                question("domain", 1, 1.5),
            ],
            &areas(&["domain"]),
            count(3),
        );

        assert_eq!(
            order(&picked),
            expected(&[("domain", 1), ("domain", 0), ("domain", 2)])
        );
    }

    #[test]
    fn questions_are_taken_in_turn_across_areas_by_their_deepest_remaining() {
        let picked = pick(
            vec![
                question("domain", 0, 1.0),
                question("domain", 1, 0.9),
                question("domain", 2, 0.8),
                question("scalability", 0, 1.5),
                question("scalability", 1, 0.1),
                question("observability", 0, 0.5),
            ],
            &areas(&["domain", "scalability", "observability"]),
            count(5),
        );

        assert_eq!(
            order(&picked),
            expected(&[
                // Round 1: 1.5, 1.0, 0.5.
                ("scalability", 0),
                ("domain", 0),
                ("observability", 0),
                // Round 2: domain's 0.9 now leads scalability's 0.1.
                ("domain", 1),
                ("scalability", 1),
            ])
        );
    }

    #[test]
    fn one_area_never_takes_every_slot() {
        let picked = pick(
            vec![
                question("domain", 0, 2.0),
                question("domain", 1, 2.0),
                question("domain", 2, 2.0),
                question("scalability", 0, 0.0),
            ],
            &areas(&["domain", "scalability"]),
            count(3),
        );

        assert_eq!(
            order(&picked),
            expected(&[("domain", 0), ("scalability", 0), ("domain", 1)])
        );
    }

    #[test]
    fn areas_whose_deepest_are_equally_deep_go_in_area_order() {
        let picked = pick(
            vec![
                question("domain", 0, 1.2),
                question("observability", 0, 1.2),
                question("scalability", 0, 1.2),
            ],
            &areas(&["scalability", "domain", "observability"]),
            count(3),
        );

        assert_eq!(
            order(&picked),
            expected(&[("scalability", 0), ("domain", 0), ("observability", 0)])
        );
    }

    #[test]
    fn an_area_not_chosen_ranks_after_the_chosen_ones_when_depths_tie() {
        let picked = pick(
            vec![
                question("security", 0, 1.0),
                question("domain", 0, 1.0),
                question("cost", 0, 1.0),
            ],
            &areas(&["domain"]),
            count(3),
        );

        assert_eq!(
            order(&picked),
            expected(&[("domain", 0), ("cost", 0), ("security", 0)])
        );
    }

    #[test]
    fn fewer_passing_than_asked_for_is_a_shortfall_of_every_one() {
        let picked = pick(
            vec![question("domain", 0, 0.0), question("scalability", 0, 2.0)],
            &areas(&["domain", "scalability"]),
            count(4),
        );

        let Picked::Shortfall {
            questions,
            asked_for,
        } = &picked
        else {
            panic!("a shortfall: {picked:?}");
        };
        assert_eq!(questions.len(), 2);
        assert_eq!(*asked_for, count(4));
        assert_eq!(
            order(&picked),
            expected(&[("scalability", 0), ("domain", 0)])
        );
    }

    #[test]
    fn none_passing_is_said_so() {
        let picked = pick(Vec::new(), &areas(&["domain"]), count(3));

        assert_eq!(picked, Picked::NonePassed);
        assert_eq!(picked.questions(), []);
    }
}
