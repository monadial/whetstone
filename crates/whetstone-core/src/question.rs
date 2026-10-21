//! Questions as the generator writes them, and the plain checks that drop a
//! malformed one before Jev sees it.
//!
//! The generator writes a [`WrittenCandidate`] for one area. [`plain_checks`]
//! turns each into a [`Candidate`], whose parts are all there and whose
//! citations all lead to lines the evidence sends, or drops it. Dropped
//! candidates are counted, never shown. A candidate becomes a question only
//! once it passes its question check (see [`show_rule`](crate::show_rule)).
//!
//! Every rule hangs off the [`QuestionType`]. Single choice is the only one
//! so far: options A, B and C, plus written reasoning.
//!
//! None of these types' `Debug` output shows a question's text, which
//! Whetstone's log never records.

use std::fmt;

use crate::area::AreaId;
use crate::connection::LineRange;
use crate::evidence::{Evidence, EvidenceConnection, EvidenceFile};

/// The form a question takes. It sets what the generator writes, what the
/// question check asks and how a response becomes outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuestionType {
    /// A single choice from options A, B and C, with written reasoning.
    SingleChoice,
}

/// One of a single-choice question's options. It reads as the learner sees
/// it:
///
/// ```
/// use whetstone_core::OptionLetter;
///
/// assert_eq!(OptionLetter::B.to_string(), "B");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OptionLetter {
    /// The first option.
    A,
    /// The second option.
    B,
    /// The third option.
    C,
}

/// A candidate question as the generator wrote it, before the plain checks.
/// Any part may be malformed: the AI crate maps the generator's reply onto it
/// as it is, with a missing part as empty text, and [`plain_checks`] decides.
#[derive(Clone, PartialEq, Eq)]
pub struct WrittenCandidate {
    /// Its question type.
    pub question_type: QuestionType,
    /// The question itself.
    pub stem: String,
    /// Its options, each with its letter and rationale.
    pub options: Vec<WrittenOption>,
    /// The letter of the option the generator marks as correct.
    pub answer_key: String,
    /// One line on what the question is getting at.
    pub why_it_matters: String,
    /// The criteria written reasoning is assessed against.
    pub rubric: Vec<WrittenCriterion>,
    /// Help that points to the evidence without revealing the answer.
    pub hint: String,
    /// The evidence the question rests on.
    pub citations: Vec<WrittenCitation>,
}

/// An option as the generator wrote it.
#[derive(Clone, PartialEq, Eq)]
pub struct WrittenOption {
    /// Its letter, such as `A`.
    pub letter: String,
    /// What it says.
    pub text: String,
    /// Why it is right or wrong.
    pub rationale: String,
}

/// A criterion as the generator wrote it.
#[derive(Clone, PartialEq, Eq)]
pub struct WrittenCriterion {
    /// What the reasoning should cover.
    pub statement: String,
    /// What meeting it looks like.
    pub met: String,
    /// What not meeting it looks like.
    pub not_met: String,
}

/// A citation as the generator wrote it: lines of a file, numbered from 1
/// as in the file, from the first to the last, both included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenCitation {
    /// The file's path, relative to the repository's root.
    pub path: String,
    /// The first line's number.
    pub first_line: usize,
    /// The last line's number.
    pub last_line: usize,
}

/// A candidate question that passed the plain checks: every part is there,
/// and every citation leads to lines the evidence sends. Its question check
/// decides whether it is shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Candidate {
    id: CandidateId,
    question_type: QuestionType,
    stem: String,
    /// Its options, in [`OptionLetter`] order.
    options: [AnswerOption; 3],
    answer_key: OptionLetter,
    why_it_matters: String,
    rubric: Vec<Criterion>,
    hint: Hint,
    citations: Vec<Citation>,
}

/// Identifies a candidate: its area, and its place among the candidates the
/// generator wrote for that area, counting from 0. Dropped candidates keep
/// their places, so the place is the order written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CandidateId {
    area: AreaId,
    written: usize,
}

/// An option of a candidate: what it says, and why it is right or wrong.
#[derive(Clone, PartialEq, Eq)]
struct AnswerOption {
    text: String,
    rationale: String,
}

/// One point of a rubric, with what meeting it and not meeting it look like.
#[derive(Clone, PartialEq, Eq)]
pub struct Criterion {
    statement: String,
    met: String,
    not_met: String,
}

/// Help that points to the evidence without revealing the answer.
#[derive(Clone, PartialEq, Eq)]
pub struct Hint(String);

/// Lines the evidence sends, which a question cites.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Citation {
    path: String,
    lines: LineRange,
}

/// What the plain checks made of one area's candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainChecks {
    passed: Vec<Candidate>,
    dropped: Vec<DropReason>,
}

/// Why the plain checks dropped a candidate: the first of these that
/// applies, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DropReason {
    /// Its options are not exactly its question type's: A, B and C, each
    /// once.
    #[error("its options are not exactly A, B and C")]
    Options,
    /// Its answer key is not one of its options.
    #[error("its answer key is not one of its options")]
    AnswerKey,
    /// Its rubric has no criterion, or more than 2.
    #[error("its rubric does not have 1 or 2 criteria")]
    RubricSize,
    /// A citation names a file or lines the evidence does not send.
    #[error("it cites lines the evidence does not send")]
    Citation,
    /// A part it needs is missing or empty.
    #[error("its {0} is missing")]
    Missing(Part),
}

/// A part of a candidate that must not be empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The stem.
    Stem,
    /// What this option says.
    Option(OptionLetter),
    /// This option's rationale.
    Rationale(OptionLetter),
    /// The "why it matters" line.
    WhyItMatters,
    /// A criterion's statement, or what meeting it or not meeting it looks
    /// like.
    Criterion,
    /// The hint.
    Hint,
    /// The citations: it needs at least one.
    Citations,
}

/// Runs the plain checks on the `written` candidates the generator wrote for
/// `area`, in the order it wrote them, against the `evidence` that was sent.
///
/// A candidate is dropped when its options are not exactly A, B and C; when
/// its answer key is not one of them; when its rubric has no criterion, or
/// more than 2; when a citation names a file or lines the evidence
/// does not send; or when a part is missing or empty, the citations
/// included. Text of only whitespace is empty.
///
/// The evidence sends each included touched file's full content, numbered
/// from its first line, and each included connection's lines, numbered as
/// in its file. A citation may span several connections of one file when
/// they send every line it names.
#[must_use]
pub fn plain_checks(
    area: &AreaId,
    written: Vec<WrittenCandidate>,
    evidence: &Evidence,
) -> PlainChecks {
    let mut passed = Vec::new();
    let mut dropped = Vec::new();
    for (index, candidate) in written.into_iter().enumerate() {
        let id = CandidateId {
            area: area.clone(),
            written: index,
        };
        match Candidate::check(id, candidate, evidence) {
            Ok(candidate) => passed.push(candidate),
            Err(reason) => dropped.push(reason),
        }
    }
    PlainChecks { passed, dropped }
}

impl Candidate {
    /// `written`, identified by `id`, if it passes the plain checks against
    /// `evidence`.
    fn check(
        id: CandidateId,
        written: WrittenCandidate,
        evidence: &Evidence,
    ) -> Result<Self, DropReason> {
        let WrittenCandidate {
            question_type,
            stem,
            options,
            answer_key,
            why_it_matters,
            rubric,
            hint,
            citations,
        } = written;
        let options = match question_type {
            QuestionType::SingleChoice => single_choice_options(options)?,
        };
        let answer_key = OptionLetter::from_written(&answer_key).ok_or(DropReason::AnswerKey)?;
        if !(1..=2).contains(&rubric.len()) {
            return Err(DropReason::RubricSize);
        }
        let citations = citations
            .into_iter()
            .map(|citation| Citation::in_evidence(citation, evidence))
            .collect::<Option<Vec<Citation>>>()
            .ok_or(DropReason::Citation)?;
        let missing = |text: &str, part| {
            if text.trim().is_empty() {
                Err(DropReason::Missing(part))
            } else {
                Ok(())
            }
        };
        missing(&stem, Part::Stem)?;
        for (letter, option) in OptionLetter::ALL.into_iter().zip(&options) {
            missing(&option.text, Part::Option(letter))?;
            missing(&option.rationale, Part::Rationale(letter))?;
        }
        missing(&why_it_matters, Part::WhyItMatters)?;
        for criterion in &rubric {
            missing(&criterion.statement, Part::Criterion)?;
            missing(&criterion.met, Part::Criterion)?;
            missing(&criterion.not_met, Part::Criterion)?;
        }
        missing(&hint, Part::Hint)?;
        if citations.is_empty() {
            return Err(DropReason::Missing(Part::Citations));
        }
        Ok(Self {
            id,
            question_type,
            stem,
            options,
            answer_key,
            why_it_matters,
            rubric: rubric
                .into_iter()
                .map(|criterion| Criterion {
                    statement: criterion.statement,
                    met: criterion.met,
                    not_met: criterion.not_met,
                })
                .collect(),
            hint: Hint(hint),
            citations,
        })
    }

    /// What identifies it: its area and its place in the order written.
    #[must_use]
    pub fn id(&self) -> &CandidateId {
        &self.id
    }

    /// The area it is about.
    #[must_use]
    pub fn area(&self) -> &AreaId {
        &self.id.area
    }

    /// Its question type.
    #[must_use]
    pub fn question_type(&self) -> QuestionType {
        self.question_type
    }

    /// The question itself.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// What the option `letter` says.
    #[must_use]
    pub fn option(&self, letter: OptionLetter) -> &str {
        &self.options[letter.index()].text
    }

    /// Why the option `letter` is right or wrong, as the generator wrote it.
    #[must_use]
    pub fn rationale(&self, letter: OptionLetter) -> &str {
        &self.options[letter.index()].rationale
    }

    /// The option the generator marks as correct.
    #[must_use]
    pub fn answer_key(&self) -> OptionLetter {
        self.answer_key
    }

    /// What the question is getting at.
    #[must_use]
    pub fn why_it_matters(&self) -> &str {
        &self.why_it_matters
    }

    /// The criteria written reasoning is assessed against, as the generator
    /// wrote them: 1 or 2. The show rule may drop some (see
    /// [`Question::rubric`](crate::Question::rubric)).
    #[must_use]
    pub fn rubric(&self) -> &[Criterion] {
        &self.rubric
    }

    /// Its hint, as the generator wrote it, before any check.
    #[must_use]
    pub fn hint(&self) -> &Hint {
        &self.hint
    }

    /// The lines of the evidence it rests on: at least one citation.
    #[must_use]
    pub fn citations(&self) -> &[Citation] {
        &self.citations
    }
}

/// `options`, by letter, if they are exactly A, B and C, each once.
fn single_choice_options(options: Vec<WrittenOption>) -> Result<[AnswerOption; 3], DropReason> {
    let mut by_letter: [Option<AnswerOption>; 3] = [None, None, None];
    for option in options {
        let slot = OptionLetter::from_written(&option.letter)
            .map(|letter| &mut by_letter[letter.index()])
            .filter(|slot| slot.is_none())
            .ok_or(DropReason::Options)?;
        *slot = Some(AnswerOption {
            text: option.text,
            rationale: option.rationale,
        });
    }
    match by_letter {
        [Some(a), Some(b), Some(c)] => Ok([a, b, c]),
        _ => Err(DropReason::Options),
    }
}

impl CandidateId {
    /// The area the candidate is about.
    #[must_use]
    pub fn area(&self) -> &AreaId {
        &self.area
    }

    /// Its place among the candidates the generator wrote for its area,
    /// counting from 0.
    #[must_use]
    pub fn written(&self) -> usize {
        self.written
    }
}

impl OptionLetter {
    /// Every option, in order.
    pub const ALL: [Self; 3] = [Self::A, Self::B, Self::C];

    /// The letter the generator wrote as `letter`, if it is exactly one.
    pub(crate) fn from_written(letter: &str) -> Option<Self> {
        match letter {
            "A" => Some(Self::A),
            "B" => Some(Self::B),
            "C" => Some(Self::C),
            _ => None,
        }
    }

    /// Its place in [`OptionLetter::ALL`].
    pub(crate) fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
        }
    }
}

impl fmt::Display for OptionLetter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
        })
    }
}

impl Criterion {
    /// What the reasoning should cover.
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    /// What meeting it looks like.
    #[must_use]
    pub fn met(&self) -> &str {
        &self.met
    }

    /// What not meeting it looks like.
    #[must_use]
    pub fn not_met(&self) -> &str {
        &self.not_met
    }
}

impl Hint {
    /// What the hint says.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl Citation {
    /// `written`, if it names lines `evidence` sends: from an included
    /// touched file's full content, or from included connections' lines.
    /// It is the plain check on every citation the generator writes, a
    /// candidate's or an explanation's.
    ///
    /// ```
    /// use whetstone_core::{
    ///     Change, Citation, ConnectedCodeLimit, Connections, Evidence, ExcludePatterns,
    ///     ReviewChoices, TouchedFile, WrittenCitation,
    /// };
    ///
    /// let change = Change::new(
    ///     "Retry failed payments".to_owned(),
    ///     Vec::new(),
    ///     "diff --git a/src/pay.rs b/src/pay.rs\n+retry\n".to_owned(),
    ///     vec![TouchedFile::modified(
    ///         "src/pay.rs".to_owned(),
    ///         "pay\n".to_owned(),
    ///         "pay\nretry\n".to_owned(),
    ///     )],
    /// );
    /// let evidence = Evidence::build(
    ///     &change,
    ///     &ExcludePatterns::default(),
    ///     &[],
    ///     &Connections::NoneFound,
    ///     ConnectedCodeLimit::effective(None, None),
    ///     &ReviewChoices::default(),
    /// );
    /// let cite = |first_line, last_line| WrittenCitation {
    ///     path: "src/pay.rs".to_owned(),
    ///     first_line,
    ///     last_line,
    /// };
    ///
    /// let sent = Citation::in_evidence(cite(1, 2), &evidence).expect("lines 1–2 are sent");
    /// assert_eq!((sent.path(), sent.lines().first(), sent.lines().last()), ("src/pay.rs", 1, 2));
    /// assert_eq!(Citation::in_evidence(cite(2, 3), &evidence), None);
    /// ```
    #[must_use]
    pub fn in_evidence(written: WrittenCitation, evidence: &Evidence) -> Option<Self> {
        let lines = LineRange::new(written.first_line, written.last_line).ok()?;
        sends(evidence, &written.path, lines).then_some(Self {
            path: written.path,
            lines,
        })
    }

    /// The cited file's path, relative to the repository's root.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The cited lines.
    #[must_use]
    pub fn lines(&self) -> LineRange {
        self.lines
    }
}

/// Whether `evidence` sends every one of `lines` of the file at `path`: from
/// an included touched file's full content, or from included connections'
/// lines.
fn sends(evidence: &Evidence, path: &str, lines: LineRange) -> bool {
    let files = evidence
        .files()
        .iter()
        .filter(|file| file.path() == path)
        .filter_map(EvidenceFile::sent)
        .map(|sent| (1, sent.content.lines().count()));
    let connections = evidence
        .connections()
        .iter()
        .filter(|connection| connection.id().path() == path)
        .filter_map(EvidenceConnection::sent)
        .map(|sent| (sent.first_line, sent.text.lines().count()));
    let mut sent: Vec<(usize, usize)> = files
        .chain(connections)
        .filter(|&(_, count)| count > 0)
        .map(|(first, count)| (first, first + count - 1))
        .collect();
    sent.sort_unstable();
    // Walk the sent ranges in order, joining those that touch, until one
    // reaches past the cited lines' end.
    let mut covered_to = lines.first() - 1;
    for (first, last) in sent {
        if first > covered_to + 1 {
            break;
        }
        covered_to = covered_to.max(last);
        if covered_to >= lines.last() {
            return true;
        }
    }
    false
}

impl PlainChecks {
    /// The candidates that passed, in the order written.
    #[must_use]
    pub fn passed(&self) -> &[Candidate] {
        &self.passed
    }

    /// The candidates that passed, in the order written.
    #[must_use]
    pub fn into_passed(self) -> Vec<Candidate> {
        self.passed
    }

    /// Why each dropped candidate was dropped, in the order written. They
    /// are counted, never shown.
    #[must_use]
    pub fn dropped(&self) -> &[DropReason] {
        &self.dropped
    }
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stem => f.write_str("stem"),
            Self::Option(letter) => write!(f, "option {letter}"),
            Self::Rationale(letter) => write!(f, "rationale for option {letter}"),
            Self::WhyItMatters => f.write_str("\"why it matters\" line"),
            Self::Criterion => f.write_str("criterion"),
            Self::Hint => f.write_str("hint"),
            Self::Citations => f.write_str("citation"),
        }
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for WrittenCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrittenCandidate")
            .field("question_type", &self.question_type)
            .field("stem_bytes", &self.stem.len())
            .field("options", &self.options)
            .field("answer_key_bytes", &self.answer_key.len())
            .field("why_it_matters_bytes", &self.why_it_matters.len())
            .field("rubric", &self.rubric)
            .field("hint_bytes", &self.hint.len())
            .field("citations", &self.citations)
            .finish()
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for WrittenOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrittenOption")
            .field("letter_bytes", &self.letter.len())
            .field("text_bytes", &self.text.len())
            .field("rationale_bytes", &self.rationale.len())
            .finish()
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for WrittenCriterion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrittenCriterion")
            .field("statement_bytes", &self.statement.len())
            .field("met_bytes", &self.met.len())
            .field("not_met_bytes", &self.not_met.len())
            .finish()
    }
}

/// Shows its identity, type, answer key and citations, and its text's
/// lengths, never the text.
impl fmt::Debug for Candidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Candidate")
            .field("id", &self.id)
            .field("question_type", &self.question_type)
            .field("stem_bytes", &self.stem.len())
            .field("options", &self.options)
            .field("answer_key", &self.answer_key)
            .field("why_it_matters_bytes", &self.why_it_matters.len())
            .field("rubric", &self.rubric)
            .field("hint", &self.hint)
            .field("citations", &self.citations)
            .finish()
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for AnswerOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnswerOption")
            .field("text_bytes", &self.text.len())
            .field("rationale_bytes", &self.rationale.len())
            .finish()
    }
}

/// Shows the text's lengths, never the text.
impl fmt::Debug for Criterion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Criterion")
            .field("statement_bytes", &self.statement.len())
            .field("met_bytes", &self.met.len())
            .field("not_met_bytes", &self.not_met.len())
            .finish()
    }
}

/// Shows the text's length, never the text.
impl fmt::Debug for Hint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hint")
            .field("bytes", &self.0.len())
            .finish()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::change::{Change, TouchedFile};
    use crate::connected_code_limit::ConnectedCodeLimit;
    use crate::connection::{Connection, ConnectionGroup, Connections, Extent, Relation, Tier};
    use crate::evidence::ReviewChoices;
    use crate::exclusion::ExcludePatterns;

    pub(crate) fn area(id: &str) -> AreaId {
        id.parse().expect("a valid area ID")
    }

    /// Evidence that sends `src/pay.rs` (3 lines) and the deleted
    /// `src/old.rs` (2 lines), but not `Cargo.lock`, with lines 10–12 and
    /// 13–14 of `src/client.rs` and the whole of `tests/pay.rs` (2 lines).
    pub(crate) fn evidence() -> Evidence {
        let lines = |count: usize| "line\n".repeat(count);
        let change = Change::new(
            "Retry failed payments".to_owned(),
            Vec::new(),
            "diff --git a/src/pay.rs b/src/pay.rs\n+retry\n\
             diff --git a/src/old.rs b/src/old.rs\n-old\n\
             diff --git a/Cargo.lock b/Cargo.lock\n+lock\n"
                .to_owned(),
            vec![
                TouchedFile::modified("src/pay.rs".to_owned(), lines(2), lines(3)),
                TouchedFile::deleted("src/old.rs".to_owned(), lines(2)),
                TouchedFile::modified("Cargo.lock".to_owned(), lines(1), lines(4)),
            ],
        );
        let connection = |path: &str, extent, group| {
            Connection::new(
                path.to_owned(),
                extent,
                group,
                Tier::High,
                vec![Relation::TestFor {
                    touched_file: "src/pay.rs".to_owned(),
                }],
                lines(20).into(),
            )
        };
        let range = |first, last| Extent::Lines(LineRange::new(first, last).expect("a range"));
        Evidence::build(
            &change,
            &ExcludePatterns::default(),
            &[],
            &Connections::Found(vec![
                connection(
                    "src/client.rs",
                    range(10, 12),
                    ConnectionGroup::ConnectedCode,
                ),
                connection(
                    "src/client.rs",
                    range(13, 14),
                    ConnectionGroup::ConnectedCode,
                ),
                Connection::new(
                    "tests/pay.rs".to_owned(),
                    Extent::WholeFile,
                    ConnectionGroup::RelatedTest,
                    Tier::High,
                    vec![Relation::TestFor {
                        touched_file: "src/pay.rs".to_owned(),
                    }],
                    lines(2).into(),
                ),
            ]),
            ConnectedCodeLimit::effective(None, None),
            &ReviewChoices::default(),
        )
    }

    pub(crate) fn cite(path: &str, first_line: usize, last_line: usize) -> WrittenCitation {
        WrittenCitation {
            path: path.to_owned(),
            first_line,
            last_line,
        }
    }

    /// A well-formed candidate whose answer key is `answer_key`.
    pub(crate) fn written(answer_key: OptionLetter) -> WrittenCandidate {
        WrittenCandidate {
            question_type: QuestionType::SingleChoice,
            stem: "Why does the retry reuse the key?".to_owned(),
            options: OptionLetter::ALL
                .into_iter()
                .map(|letter| WrittenOption {
                    letter: letter.to_string(),
                    text: format!("Option {letter}"),
                    rationale: format!("Rationale {letter}"),
                })
                .collect(),
            answer_key: answer_key.to_string(),
            why_it_matters: "A retry must not charge twice.".to_owned(),
            rubric: vec![criterion(), second_criterion()],
            hint: "Look at how the key is made.".to_owned(),
            citations: vec![cite("src/pay.rs", 1, 3)],
        }
    }

    fn criterion() -> WrittenCriterion {
        WrittenCriterion {
            statement: "Links the key to deduplication".to_owned(),
            met: "Says the provider deduplicates by key".to_owned(),
            not_met: "Doesn't mention deduplication".to_owned(),
        }
    }

    fn second_criterion() -> WrittenCriterion {
        WrittenCriterion {
            statement: "Says the provider charges once".to_owned(),
            met: "Says one charge is applied".to_owned(),
            not_met: "Doesn't say how often it charges".to_owned(),
        }
    }

    /// The plain checks' verdict on `candidate` alone.
    fn check(candidate: WrittenCandidate) -> Result<Candidate, DropReason> {
        let checked = plain_checks(&area("domain"), vec![candidate], &evidence());
        match (checked.passed(), checked.dropped()) {
            ([candidate], []) => Ok(candidate.clone()),
            ([], [reason]) => Err(*reason),
            _ => panic!("one candidate is either passed or dropped"),
        }
    }

    #[test]
    fn a_well_formed_candidate_passes_with_every_part() {
        let candidate = check(written(OptionLetter::B)).expect("well formed");

        assert_eq!(candidate.area(), &area("domain"));
        assert_eq!(candidate.question_type(), QuestionType::SingleChoice);
        assert_eq!(candidate.stem(), "Why does the retry reuse the key?");
        assert_eq!(candidate.option(OptionLetter::C), "Option C");
        assert_eq!(candidate.rationale(OptionLetter::A), "Rationale A");
        assert_eq!(candidate.answer_key(), OptionLetter::B);
        assert_eq!(candidate.why_it_matters(), "A retry must not charge twice.");
        assert_eq!(candidate.rubric().len(), 2);
        assert_eq!(
            candidate.rubric()[0].statement(),
            "Links the key to deduplication"
        );
        assert_eq!(
            candidate.rubric()[0].met(),
            "Says the provider deduplicates by key"
        );
        assert_eq!(
            candidate.rubric()[0].not_met(),
            "Doesn't mention deduplication"
        );
        assert_eq!(candidate.hint().text(), "Look at how the key is made.");
        assert_eq!(candidate.citations()[0].path(), "src/pay.rs");
        assert_eq!(
            candidate.citations()[0].lines(),
            LineRange::new(1, 3).expect("a range")
        );
    }

    #[test]
    fn options_written_in_any_order_are_kept_by_letter() {
        let mut candidate = written(OptionLetter::A);
        candidate.options.reverse();

        let candidate = check(candidate).expect("A, B and C, in another order");

        assert_eq!(candidate.option(OptionLetter::A), "Option A");
        assert_eq!(candidate.rationale(OptionLetter::C), "Rationale C");
    }

    #[test]
    fn a_candidate_whose_options_are_not_exactly_a_b_and_c_is_dropped() {
        let mut two = written(OptionLetter::A);
        two.options.pop();
        let mut four = written(OptionLetter::A);
        four.options.push(four.options[0].clone());
        let mut repeated = written(OptionLetter::A);
        repeated.options[2].letter = "B".to_owned();
        let mut d = written(OptionLetter::A);
        d.options[2].letter = "D".to_owned();
        let mut lowercase = written(OptionLetter::A);
        lowercase.options[1].letter = "b".to_owned();

        for candidate in [two, four, repeated, d, lowercase] {
            assert_eq!(check(candidate), Err(DropReason::Options));
        }
    }

    #[test]
    fn a_candidate_whose_answer_key_is_not_one_of_its_options_is_dropped() {
        for key in ["D", "a", "", "A or B"] {
            let mut candidate = written(OptionLetter::A);
            candidate.answer_key = key.to_owned();

            assert_eq!(check(candidate), Err(DropReason::AnswerKey), "{key:?}");
        }
    }

    #[test]
    fn a_rubric_has_1_or_2_criteria() {
        for (count, verdict) in [
            (0, Err(DropReason::RubricSize)),
            (1, Ok(())),
            (2, Ok(())),
            (3, Err(DropReason::RubricSize)),
        ] {
            let mut candidate = written(OptionLetter::A);
            candidate.rubric = vec![criterion(); count];

            assert_eq!(check(candidate).map(|_| ()), verdict, "{count}");
        }
    }

    #[test]
    fn a_citation_must_name_lines_the_evidence_sends() {
        for (citation, verdict) in [
            (cite("src/pay.rs", 2, 2), Ok(())),
            (cite("src/old.rs", 1, 2), Ok(())),
            (cite("src/client.rs", 10, 12), Ok(())),
            (cite("src/client.rs", 11, 14), Ok(())),
            (cite("tests/pay.rs", 1, 2), Ok(())),
            (cite("src/pay.rs", 2, 4), Err(DropReason::Citation)),
            (cite("src/old.rs", 3, 3), Err(DropReason::Citation)),
            (cite("src/client.rs", 9, 10), Err(DropReason::Citation)),
            (cite("src/client.rs", 14, 15), Err(DropReason::Citation)),
            (cite("tests/pay.rs", 3, 3), Err(DropReason::Citation)),
            (cite("Cargo.lock", 1, 1), Err(DropReason::Citation)),
            (cite("src/other.rs", 1, 1), Err(DropReason::Citation)),
            (cite("src/pay.rs", 0, 1), Err(DropReason::Citation)),
            (cite("src/pay.rs", 3, 2), Err(DropReason::Citation)),
        ] {
            let mut candidate = written(OptionLetter::A);
            candidate.citations.push(citation.clone());

            assert_eq!(check(candidate).map(|_| ()), verdict, "{citation:?}");
        }
    }

    #[test]
    fn a_candidate_missing_a_part_is_dropped() {
        type Edit = fn(&mut WrittenCandidate);
        let cases: [(Edit, Part); 9] = [
            (|c| c.stem = " \n".to_owned(), Part::Stem),
            (|c| c.options[1].text.clear(), Part::Option(OptionLetter::B)),
            (
                |c| c.options[2].rationale.clear(),
                Part::Rationale(OptionLetter::C),
            ),
            (|c| c.why_it_matters.clear(), Part::WhyItMatters),
            (|c| c.rubric[1].statement.clear(), Part::Criterion),
            (|c| c.rubric[0].met.clear(), Part::Criterion),
            (|c| c.rubric[0].not_met = "\t".to_owned(), Part::Criterion),
            (|c| c.hint.clear(), Part::Hint),
            (|c| c.citations.clear(), Part::Citations),
        ];
        for (edit, part) in cases {
            let mut candidate = written(OptionLetter::A);
            edit(&mut candidate);

            assert_eq!(check(candidate), Err(DropReason::Missing(part)), "{part}");
        }
    }

    #[test]
    fn dropped_candidates_are_counted_and_passed_ones_keep_their_place_written() {
        let mut malformed = written(OptionLetter::A);
        malformed.answer_key = "D".to_owned();

        let checked = plain_checks(
            &area("domain"),
            vec![
                malformed,
                written(OptionLetter::B),
                written(OptionLetter::C),
            ],
            &evidence(),
        );

        assert_eq!(checked.dropped(), [DropReason::AnswerKey]);
        let places: Vec<usize> = checked
            .passed()
            .iter()
            .map(|candidate| candidate.id().written())
            .collect();
        assert_eq!(places, [1, 2]);
        assert_eq!(checked.into_passed().len(), 2);
    }

    #[test]
    fn debug_output_never_shows_a_questions_text() {
        let written = written(OptionLetter::A);
        let candidate = check(written.clone()).expect("well formed");

        for debug in [format!("{written:?}"), format!("{candidate:?}")] {
            for text in [
                "retry reuse",
                "Option A",
                "Rationale",
                "charge twice",
                "dedup",
                "Look at",
            ] {
                assert!(!debug.contains(text), "{debug}");
            }
        }
    }
}
