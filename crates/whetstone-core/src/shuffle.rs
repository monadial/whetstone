//! The option shuffle: after the generator writes a candidate and before
//! it is checked, its options move to an order of their own, so that where
//! the answer key sits carries no signal. The generator tends to put the
//! correct option first.
//!
//! The order is decided by the candidate's own content, so the same
//! candidate always gets the same order: on every platform, in every
//! release, and in tests and recordings alike.

use std::mem;

use crate::question::{WrittenCandidate, WrittenOption};

/// A single-choice question's letters, in the order the learner reads them.
const LETTERS: [&str; 3] = ["A", "B", "C"];

/// Every order of three options. Each lists, letter by letter, the place in
/// letter order of the option written there.
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

/// `written` with its options shuffled: each option's text and rationale
/// move together to a new letter, the letters still read A, B and C in
/// order, and the answer key names the letter its option moved to. Which
/// order a candidate gets depends only on its stem, its options and its
/// answer key.
///
/// A candidate whose options aren't exactly A, B and C, each once, or whose
/// answer key isn't one of them, is returned as written, for
/// [`plain_checks`](crate::plain_checks) to drop.
///
/// ```
/// use whetstone_core::{QuestionType, WrittenCandidate, WrittenOption, shuffle_options};
///
/// let option = |letter: &str, text: &str| WrittenOption {
///     letter: letter.to_owned(),
///     text: text.to_owned(),
///     rationale: format!("Why {text} is right or wrong"),
/// };
/// let written = WrittenCandidate {
///     question_type: QuestionType::SingleChoice,
///     stem: "What does the retry send?".to_owned(),
///     options: vec![option("A", "the same key"), option("B", "a new key"), option("C", "no key")],
///     answer_key: "A".to_owned(),
///     why_it_matters: String::new(),
///     rubric: Vec::new(),
///     hint: String::new(),
///     citations: Vec::new(),
/// };
///
/// let shuffled = shuffle_options(written.clone());
///
/// let correct = shuffled.options.iter().find(|option| option.letter == shuffled.answer_key);
/// assert_eq!(correct.map(|option| option.text.as_str()), Some("the same key"));
/// assert_eq!(shuffle_options(written), shuffled);
/// ```
#[must_use]
pub fn shuffle_options(mut written: WrittenCandidate) -> WrittenCandidate {
    let Some(written_at) = in_letter_order(&written.options) else {
        return written;
    };
    let Some(answer_key) = LETTERS
        .iter()
        .position(|&letter| letter == written.answer_key)
    else {
        return written;
    };
    let order = ORDERS[order_of(&written, written_at)];
    let mut options: Vec<Option<WrittenOption>> = mem::take(&mut written.options)
        .into_iter()
        .map(Some)
        .collect();
    written.options = order
        .iter()
        .zip(LETTERS)
        .map(|(&from, letter)| {
            let mut option = options[written_at[from]]
                .take()
                .expect("each option moves to one letter");
            letter.clone_into(&mut option.letter);
            option
        })
        .collect();
    let moved_to = order
        .iter()
        .position(|&from| from == answer_key)
        .expect("every option gets a letter");
    LETTERS[moved_to].clone_into(&mut written.answer_key);
    written
}

/// Where each of A, B and C is among `options`, if they are exactly those
/// three, each once.
fn in_letter_order(options: &[WrittenOption]) -> Option<[usize; 3]> {
    let mut places = [None; 3];
    for (at, option) in options.iter().enumerate() {
        let letter = LETTERS.iter().position(|&letter| letter == option.letter)?;
        if places[letter].replace(at).is_some() {
            return None;
        }
    }
    match places {
        [Some(a), Some(b), Some(c)] => Some([a, b, c]),
        _ => None,
    }
}

/// Which of [`ORDERS`] `written` gets: from FNV-1a over its stem, its
/// options' text and rationales in letter order, and its answer key, whose
/// bits are then mixed (the `SplitMix64` finaliser) so every bit counts. Both
/// are fixed by their definitions, so the order never changes with the
/// platform or the release, as the standard library's hasher may.
fn order_of(written: &WrittenCandidate, written_at: [usize; 3]) -> usize {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let options = written_at.iter().flat_map(|&at| {
        let option = &written.options[at];
        [option.text.as_str(), option.rationale.as_str()]
    });
    let hash = [written.stem.as_str()]
        .into_iter()
        .chain(options)
        .chain([written.answer_key.as_str()])
        // 0xFF never occurs in UTF-8, so it ends each part unambiguously.
        .flat_map(|part| part.bytes().chain([0xFF]))
        .fold(FNV_OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        });
    let mixed = (hash ^ (hash >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let mixed = mixed ^ (mixed >> 31);
    usize::try_from(mixed % ORDERS.len() as u64).expect("an index below 6 fits a usize")
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::question::{
        DropReason, QuestionType, WrittenCitation, WrittenCriterion, WrittenOption, plain_checks,
        tests::{area, evidence},
    };

    /// A candidate asking `stem` whose options are written in the order of
    /// `letters`, each option's text and rationale naming the option, and
    /// whose answer key is the option written first.
    fn written(stem: &str, letters: [&str; 3], answer_key: &str) -> WrittenCandidate {
        WrittenCandidate {
            question_type: QuestionType::SingleChoice,
            stem: stem.to_owned(),
            options: letters
                .iter()
                .map(|letter| WrittenOption {
                    letter: (*letter).to_owned(),
                    text: format!("Option {letter}"),
                    rationale: format!("Why option {letter} is right or wrong"),
                })
                .collect(),
            answer_key: answer_key.to_owned(),
            why_it_matters: "Why it matters".to_owned(),
            rubric: vec![WrittenCriterion {
                statement: "Covers it".to_owned(),
                met: "Does".to_owned(),
                not_met: "Doesn't".to_owned(),
            }],
            hint: "Look at the retry".to_owned(),
            citations: vec![WrittenCitation {
                path: "src/pay.rs".to_owned(),
                first_line: 1,
                last_line: 2,
            }],
        }
    }

    /// The letter the option first written as `A` has after the shuffle.
    fn where_a_went(shuffled: &WrittenCandidate) -> &str {
        &shuffled
            .options
            .iter()
            .find(|option| option.text == "Option A")
            .expect("the option written as A is still there")
            .letter
    }

    #[test]
    fn the_answer_key_can_end_up_at_each_letter() {
        let landed: BTreeSet<String> = (0..30)
            .map(|number| {
                shuffle_options(written(
                    &format!("Question {number}?"),
                    ["A", "B", "C"],
                    "A",
                ))
            })
            .map(|shuffled| {
                assert_eq!(shuffled.answer_key, where_a_went(&shuffled));
                shuffled.answer_key
            })
            .collect();

        assert_eq!(landed, BTreeSet::from(["A", "B", "C"].map(str::to_owned)));
    }

    #[test]
    fn the_same_candidate_always_gets_the_same_order() {
        let candidate = written("Why does the retry keep its key?", ["A", "B", "C"], "A");

        let first = shuffle_options(candidate.clone());

        assert_eq!(shuffle_options(candidate.clone()), first);
        // The order an independent FNV-1a and SplitMix64 give, so a change
        // to how the order is decided shows: it would reorder the options
        // of every recorded candidate.
        let letters: Vec<(&str, &str)> = first
            .options
            .iter()
            .map(|option| (option.letter.as_str(), option.text.as_str()))
            .collect();
        assert_eq!(
            letters,
            [("A", "Option C"), ("B", "Option B"), ("C", "Option A")]
        );
        assert_eq!(first.answer_key, "C");
    }

    #[test]
    fn rationales_and_the_answer_key_follow_their_options_and_the_letters_still_read_a_b_c() {
        for number in 0..30 {
            let shuffled = shuffle_options(written(
                &format!("Question {number}?"),
                ["C", "A", "B"],
                "B",
            ));

            let letters: Vec<&str> = shuffled
                .options
                .iter()
                .map(|option| option.letter.as_str())
                .collect();
            assert_eq!(letters, ["A", "B", "C"]);
            for option in &shuffled.options {
                let written_as = option
                    .text
                    .strip_prefix("Option ")
                    .expect("each text names its option");
                assert_eq!(
                    option.rationale,
                    format!("Why option {written_as} is right or wrong")
                );
            }
            let correct = shuffled
                .options
                .iter()
                .find(|option| option.letter == shuffled.answer_key)
                .expect("the answer key names an option");
            assert_eq!(correct.text, "Option B");
        }
    }

    #[test]
    fn the_answer_key_lands_at_each_letter_about_as_often() {
        let mut landed: BTreeMap<String, usize> = BTreeMap::new();
        let mut orders = BTreeSet::new();
        for number in 0..300 {
            let shuffled = shuffle_options(written(
                &format!("Question {number}?"),
                ["A", "B", "C"],
                "A",
            ));
            *landed
                .entry(where_a_went(&shuffled).to_owned())
                .or_default() += 1;
            orders.insert(
                shuffled
                    .options
                    .iter()
                    .map(|option| option.text.clone())
                    .collect::<Vec<_>>(),
            );
        }

        assert_eq!(orders.len(), 6, "every order of three options occurs");
        for letter in ["A", "B", "C"] {
            let count = landed.get(letter).copied().unwrap_or_default();
            assert!(
                (80..=120).contains(&count),
                "the key landed at {letter} {count} times in 300"
            );
        }
    }

    #[test]
    fn a_candidate_without_exactly_a_b_and_c_or_with_another_answer_key_is_left_as_written() {
        let repeated = written("Which?", ["A", "A", "C"], "A");
        let other_letter = written("Which?", ["A", "B", "D"], "A");
        let mut two = written("Which?", ["A", "B", "C"], "A");
        two.options.pop();
        let other_key = written("Which?", ["A", "B", "C"], "D");

        for malformed in [repeated, other_letter, two, other_key] {
            assert_eq!(shuffle_options(malformed.clone()), malformed);
        }
        let dropped = plain_checks(
            &area("domain"),
            vec![shuffle_options(written("Which?", ["A", "B", "C"], "D"))],
            &evidence(),
        );
        assert_eq!(dropped.dropped(), [DropReason::AnswerKey]);
    }
}
