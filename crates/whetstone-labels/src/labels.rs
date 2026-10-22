//! The labelled examples' format: TOML files beside the showcase project, in
//! `showcase/labels/`, whose `README.md` is the maintainer's guide to it.
//!
//! A file holds any number of examples of four kinds, each an array of
//! tables: `[[gate]]`, `[[candidate]]`, `[[response]]` and
//! `[[explanation]]`. Every example has an `id`, unique across all files;
//! who labelled it, as `by`, and when, as `on`; an optional `note`; and its
//! `labels`, each `"yes"` or `"no"`: what Jev should answer. A label may
//! name its own `by` and `on`, as `{ label = "no", by = "…", on = … }`, when
//! someone other than the example's labeller set it.
//!
//! A candidate example may also be a learner's vote, which Whetstone's
//! review of withheld questions saves in the learner's labels file:
//! `source = "learner vote"`, labelling only the check voted on, and, when
//! the vote is about a commit of the learner's own repository rather than
//! the showcase project, that commit's full ID as `commit_id`.
//!
//! [`Labels::parse`] reads the files and checks what each says on its own.
//! Whether an example fits the showcase project, such as its commit, its
//! area, its citations and which labels it needs, is checked when it is
//! prepared ([`Labels::prepare`](crate::Labels::prepare)).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr as _;

use serde::Deserialize;
use serde::de::{self, Deserializer, IntoDeserializer, MapAccess, Visitor};
use toml::Spanned;
use toml::value::Datetime;
use whetstone_core::{
    AreaId, CommitId, CriterionOutcome, InvalidAreaId, InvalidCommitId, QuestionType,
    WrittenCandidate, WrittenCitation, WrittenCriterion, WrittenOption,
};

/// What a label says Jev should answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expected {
    /// Jev should answer yes: the change is worth asking about, the check
    /// holds, the option is right, the reasoning meets the criterion.
    Yes,
    /// Jev should answer no.
    No,
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Yes => "yes",
            Self::No => "no",
        })
    }
}

/// Who made a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Labeller {
    /// An agent, written `by = "agent"`: a seed for a person to review.
    Agent,
    /// A person, by the name they gave.
    Person(String),
}

impl fmt::Display for Labeller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agent => f.write_str("agent"),
            Self::Person(name) => f.write_str(name),
        }
    }
}

/// Where a candidate example comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Written or pasted in to be labelled, by a person or an agent: it
    /// labels everything its call asks.
    Labelled,
    /// A learner's vote on one check that withheld a candidate the generator
    /// wrote, as Whetstone's review of withheld questions saves it: it
    /// labels only that check.
    LearnerVote,
}

/// The day a label was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LabelDate {
    year: u16,
    month: u8,
    day: u8,
}

impl fmt::Display for LabelDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// One label: what Jev should answer, who said so and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    expected: Expected,
    by: Labeller,
    on: LabelDate,
}

impl Label {
    /// What Jev should answer.
    #[must_use]
    pub fn expected(&self) -> Expected {
        self.expected
    }

    /// Who made it.
    #[must_use]
    pub fn by(&self) -> &Labeller {
        &self.by
    }

    /// When it was made.
    #[must_use]
    pub fn on(&self) -> LabelDate {
        self.on
    }
}

/// Where an example starts: its file and line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    file: String,
    line: usize,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, line {}", self.file, self.line)
    }
}

/// An example's ID: unique across all the files, and how a response or
/// explanation example names the candidate example it is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExampleId(String);

impl ExampleId {
    /// The ID `id`.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The ID as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExampleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What every example has: where it is, its ID, its note and its labels,
/// by key, in key order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    place: Place,
    id: ExampleId,
    note: Option<String>,
    labels: Vec<(String, Label)>,
}

impl Example {
    /// Where it starts.
    #[must_use]
    pub fn place(&self) -> &Place {
        &self.place
    }

    /// Its ID, unique across all the files.
    #[must_use]
    pub fn id(&self) -> &ExampleId {
        &self.id
    }

    /// Why it exists, for the people who review it.
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Its labels, by key, in key order.
    #[must_use]
    pub fn labels(&self) -> &[(String, Label)] {
        &self.labels
    }
}

/// A relevance gate example: for one showcase commit, whether it is worth
/// asking about in each labelled area, keyed by area ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateExample {
    example: Example,
    commit: String,
}

impl GateExample {
    /// Its ID, place, note and labels.
    #[must_use]
    pub fn example(&self) -> &Example {
        &self.example
    }

    /// The subject of the showcase commit it is about.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }
}

/// A candidate example: a question as the generator would write it, about
/// one showcase commit and area, with a label for each of its question
/// check's questions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateExample {
    example: Example,
    source: Source,
    commit: String,
    commit_id: Option<CommitId>,
    area: AreaId,
    written: WrittenCandidate,
}

impl CandidateExample {
    /// Its ID, place, note and labels.
    #[must_use]
    pub fn example(&self) -> &Example {
        &self.example
    }

    /// Where it comes from: labelled, or a learner's vote.
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    /// The subject of the commit it is about: a showcase commit's, unless
    /// [`commit_id`](Self::commit_id) names one of the learner's own.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// The ID of the commit of the learner's own repository a learner's
    /// vote is about, or none when it is about the showcase project.
    #[must_use]
    pub fn commit_id(&self) -> Option<&CommitId> {
        self.commit_id.as_ref()
    }

    /// The ID of the area it is about.
    #[must_use]
    pub fn area(&self) -> &AreaId {
        &self.area
    }

    /// The candidate, as written.
    #[must_use]
    pub fn written(&self) -> &WrittenCandidate {
        &self.written
    }
}

/// A written response example: a learner's reasoning for a candidate
/// example's question, with a label for each criterion of its rubric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseExample {
    example: Example,
    candidate: ExampleId,
    reasoning: String,
}

impl ResponseExample {
    /// Its ID, place, note and labels.
    #[must_use]
    pub fn example(&self) -> &Example {
        &self.example
    }

    /// The ID of the candidate example whose question it answers.
    #[must_use]
    pub fn candidate(&self) -> &ExampleId {
        &self.candidate
    }

    /// The reasoning, as the learner wrote it.
    #[must_use]
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }
}

/// One paragraph of an explanation example, as the generator would write
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleParagraph {
    /// The criterion it explains, counting from 1, as its labels name it.
    pub criterion: usize,
    /// That criterion's outcome: not met, or needs review.
    pub outcome: CriterionOutcome,
    /// The paragraph.
    pub text: String,
    /// What it cites.
    pub citations: Vec<WrittenCitation>,
}

/// An explanation example: paragraphs explaining what a response to a
/// candidate example's question missed, with two labels for each paragraph:
/// whether the evidence supports it, and whether it agrees with its
/// criterion's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplanationExample {
    example: Example,
    candidate: ExampleId,
    paragraphs: Vec<ExampleParagraph>,
}

impl ExplanationExample {
    /// Its ID, place, note and labels.
    #[must_use]
    pub fn example(&self) -> &Example {
        &self.example
    }

    /// The ID of the candidate example whose question it explains.
    #[must_use]
    pub fn candidate(&self) -> &ExampleId {
        &self.candidate
    }

    /// Its paragraphs, in the order written.
    #[must_use]
    pub fn paragraphs(&self) -> &[ExampleParagraph] {
        &self.paragraphs
    }
}

/// Every labelled example, from every file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    gate: Vec<GateExample>,
    candidates: Vec<CandidateExample>,
    responses: Vec<ResponseExample>,
    explanations: Vec<ExplanationExample>,
}

/// A labelled-example file that can't be read as the format says.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelError {
    /// The file isn't TOML, or a value isn't what its key needs.
    #[error("{file}, line {line}: {message}")]
    Toml {
        file: String,
        line: usize,
        message: String,
    },
    /// A required text is empty.
    #[error("{place}: `{field}` is empty")]
    Empty { place: Place, field: &'static str },
    /// `on` is a time or a date with a time, not a date.
    #[error("{place}: `on` must be a date, such as 2026-10-21")]
    NotADate { place: Place },
    /// An example has no labels.
    #[error("{place}: `{id}` has no labels")]
    NoLabels { place: Place, id: ExampleId },
    /// Two examples have the same ID.
    #[error("{second}: `{id}` is already the ID of the example at {first}")]
    Duplicate {
        id: ExampleId,
        first: Place,
        second: Place,
    },
    /// A candidate example's `area` isn't an area ID.
    #[error("{place}: the candidate's `area` is not valid")]
    NotAnAreaId {
        place: Place,
        #[source]
        source: InvalidAreaId,
    },
    /// A candidate example that isn't a learner's vote names a commit of a
    /// learner's own repository.
    #[error(
        "{place}: `commit_id` names a commit of a learner's own repository, which only a learner vote (`source = \"learner vote\"`) is about"
    )]
    NotAVote { place: Place },
    /// A learner's vote's `commit_id` isn't a commit's full ID.
    #[error("{place}: the vote's `commit_id` is not a commit's full ID")]
    NotACommitId {
        place: Place,
        #[source]
        source: InvalidCommitId,
    },
}

impl Labels {
    /// Reads the labelled examples in `files`, each its name and its text,
    /// in the order given.
    ///
    /// Fails on the first file that isn't TOML or holds a key the format
    /// doesn't have, a value of the wrong kind, a label that is neither
    /// `"yes"` nor `"no"`, an `on` that isn't a date, an empty ID, `by` or
    /// text, an example with no labels, or an ID already used.
    pub fn parse<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, LabelError> {
        let mut labels = Self::default();
        for (file, text) in files {
            let raw: RawFile = toml::from_str(text).map_err(|error| LabelError::Toml {
                file: file.to_owned(),
                line: line_at(text, error.span().map_or(0, |span| span.start)),
                message: error.message().trim().to_owned(),
            })?;
            let place = |span: std::ops::Range<usize>| Place {
                file: file.to_owned(),
                line: line_at(text, span.start),
            };
            for raw in raw.gate {
                let place = place(raw.span());
                labels.gate.push(raw.into_inner().into_example(&place)?);
            }
            for raw in raw.candidate {
                let place = place(raw.span());
                labels
                    .candidates
                    .push(raw.into_inner().into_example(&place)?);
            }
            for raw in raw.response {
                let place = place(raw.span());
                labels
                    .responses
                    .push(raw.into_inner().into_example(&place)?);
            }
            for raw in raw.explanation {
                let place = place(raw.span());
                labels
                    .explanations
                    .push(raw.into_inner().into_example(&place)?);
            }
        }
        labels.check_ids()?;
        Ok(labels)
    }

    /// The relevance gate examples.
    #[must_use]
    pub fn gate(&self) -> &[GateExample] {
        &self.gate
    }

    /// The candidate examples.
    #[must_use]
    pub fn candidates(&self) -> &[CandidateExample] {
        &self.candidates
    }

    /// The written response examples.
    #[must_use]
    pub fn responses(&self) -> &[ResponseExample] {
        &self.responses
    }

    /// The explanation examples.
    #[must_use]
    pub fn explanations(&self) -> &[ExplanationExample] {
        &self.explanations
    }

    /// Fails on the first example, by kind and then in file order, whose ID
    /// an earlier one has.
    fn check_ids(&self) -> Result<(), LabelError> {
        let mut ids: BTreeMap<&ExampleId, &Place> = BTreeMap::new();
        let examples = (self.gate.iter().map(GateExample::example))
            .chain(self.candidates.iter().map(CandidateExample::example))
            .chain(self.responses.iter().map(ResponseExample::example))
            .chain(self.explanations.iter().map(ExplanationExample::example));
        for example in examples {
            if let Some(first) = ids.insert(&example.id, &example.place) {
                return Err(LabelError::Duplicate {
                    id: example.id.clone(),
                    first: first.clone(),
                    second: example.place.clone(),
                });
            }
        }
        Ok(())
    }
}

/// The 1-based line of `text` that byte `offset` is on.
fn line_at(text: &str, offset: usize) -> usize {
    1 + text
        .bytes()
        .take(offset)
        .filter(|&byte| byte == b'\n')
        .count()
}

fn non_empty(text: String, place: &Place, field: &'static str) -> Result<String, LabelError> {
    if text.trim().is_empty() {
        Err(LabelError::Empty {
            place: place.clone(),
            field,
        })
    } else {
        Ok(text)
    }
}

fn date(on: Datetime, place: &Place) -> Result<LabelDate, LabelError> {
    match on {
        Datetime {
            date: Some(date),
            time: None,
            offset: None,
        } => Ok(LabelDate {
            year: date.year,
            month: date.month,
            day: date.day,
        }),
        _ => Err(LabelError::NotADate {
            place: place.clone(),
        }),
    }
}

/// One file, as TOML.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    gate: Vec<Spanned<RawGate>>,
    #[serde(default)]
    candidate: Vec<Spanned<RawCandidate>>,
    #[serde(default)]
    response: Vec<Spanned<RawResponse>>,
    #[serde(default)]
    explanation: Vec<Spanned<RawExplanation>>,
}

/// The keys every example has, which each kind's table repeats.
#[derive(Debug)]
struct RawCommon {
    id: String,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
}

impl RawCommon {
    fn new(
        id: String,
        by: String,
        on: Datetime,
        note: Option<String>,
        labels: BTreeMap<String, RawLabel>,
    ) -> Self {
        Self {
            id,
            by,
            on,
            note,
            labels,
        }
    }

    fn into_example(self, place: &Place) -> Result<Example, LabelError> {
        let id = ExampleId(non_empty(self.id, place, "id")?);
        let by = labeller(self.by, place)?;
        let on = date(self.on, place)?;
        if self.labels.is_empty() {
            return Err(LabelError::NoLabels {
                place: place.clone(),
                id,
            });
        }
        let labels = self
            .labels
            .into_iter()
            .map(|(key, label)| {
                let label = match label {
                    RawLabel::Plain(expected) => Label {
                        expected,
                        by: by.clone(),
                        on,
                    },
                    RawLabel::Own(own) => Label {
                        expected: own.label,
                        by: labeller(own.by, place)?,
                        on: date(own.on, place)?,
                    },
                };
                Ok((key, label))
            })
            .collect::<Result<_, LabelError>>()?;
        Ok(Example {
            place: place.clone(),
            id,
            note: self.note,
            labels,
        })
    }
}

fn labeller(by: String, place: &Place) -> Result<Labeller, LabelError> {
    let by = non_empty(by, place, "by")?;
    Ok(if by == "agent" {
        Labeller::Agent
    } else {
        Labeller::Person(by)
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGate {
    id: String,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
    commit: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCandidate {
    id: String,
    source: Option<RawSource>,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
    commit: String,
    commit_id: Option<String>,
    area: String,
    stem: String,
    options: Vec<RawOption>,
    answer_key: String,
    why_it_matters: String,
    rubric: Vec<RawCriterion>,
    hint: String,
    citations: Vec<RawCitation>,
}

/// Where a candidate example says it comes from, when it says.
#[derive(Debug, Clone, Copy, Deserialize)]
enum RawSource {
    #[serde(rename = "learner vote")]
    LearnerVote,
}

/// A candidate's parts, as the generator writes them.
#[derive(Debug)]
struct RawQuestion {
    stem: String,
    options: Vec<RawOption>,
    answer_key: String,
    why_it_matters: String,
    rubric: Vec<RawCriterion>,
    hint: String,
    citations: Vec<RawCitation>,
}

impl RawQuestion {
    fn into_written(self) -> WrittenCandidate {
        WrittenCandidate {
            question_type: QuestionType::SingleChoice,
            stem: self.stem,
            options: self
                .options
                .into_iter()
                .map(|option| WrittenOption {
                    letter: option.letter,
                    text: option.text,
                    rationale: option.rationale,
                })
                .collect(),
            answer_key: self.answer_key,
            why_it_matters: self.why_it_matters,
            rubric: self
                .rubric
                .into_iter()
                .map(|criterion| WrittenCriterion {
                    statement: criterion.statement,
                    met: criterion.met,
                    not_met: criterion.not_met,
                })
                .collect(),
            hint: self.hint,
            citations: self
                .citations
                .into_iter()
                .map(RawCitation::into_written)
                .collect(),
        }
    }
}

impl RawGate {
    fn into_example(self, place: &Place) -> Result<GateExample, LabelError> {
        let Self {
            id,
            by,
            on,
            note,
            labels,
            commit,
        } = self;
        Ok(GateExample {
            example: RawCommon::new(id, by, on, note, labels).into_example(place)?,
            commit: non_empty(commit, place, "commit")?,
        })
    }
}

impl RawCandidate {
    fn into_example(self, place: &Place) -> Result<CandidateExample, LabelError> {
        let Self {
            id,
            source,
            by,
            on,
            note,
            labels,
            commit,
            commit_id,
            area,
            stem,
            options,
            answer_key,
            why_it_matters,
            rubric,
            hint,
            citations,
        } = self;
        let question = RawQuestion {
            stem,
            options,
            answer_key,
            why_it_matters,
            rubric,
            hint,
            citations,
        };
        let source = match source {
            None => Source::Labelled,
            Some(RawSource::LearnerVote) => Source::LearnerVote,
        };
        if source == Source::Labelled && commit_id.is_some() {
            return Err(LabelError::NotAVote {
                place: place.clone(),
            });
        }
        Ok(CandidateExample {
            example: RawCommon::new(id, by, on, note, labels).into_example(place)?,
            source,
            commit: non_empty(commit, place, "commit")?,
            commit_id: commit_id
                .map(|id| {
                    // The short form is never shown: a vote names the commit
                    // by its full ID.
                    CommitId::new(&id, 7).map_err(|source| LabelError::NotACommitId {
                        place: place.clone(),
                        source,
                    })
                })
                .transpose()?,
            area: AreaId::from_str(&area).map_err(|source| LabelError::NotAnAreaId {
                place: place.clone(),
                source,
            })?,
            written: question.into_written(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOption {
    letter: String,
    text: String,
    rationale: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCriterion {
    statement: String,
    met: String,
    not_met: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCitation {
    path: String,
    first_line: usize,
    last_line: usize,
}

impl RawCitation {
    fn into_written(self) -> WrittenCitation {
        WrittenCitation {
            path: self.path,
            first_line: self.first_line,
            last_line: self.last_line,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResponse {
    id: String,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
    candidate: String,
    reasoning: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExplanation {
    id: String,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
    candidate: String,
    paragraphs: Vec<RawParagraph>,
}

impl RawResponse {
    fn into_example(self, place: &Place) -> Result<ResponseExample, LabelError> {
        let Self {
            id,
            by,
            on,
            note,
            labels,
            candidate,
            reasoning,
        } = self;
        Ok(ResponseExample {
            example: RawCommon::new(id, by, on, note, labels).into_example(place)?,
            candidate: ExampleId(non_empty(candidate, place, "candidate")?),
            reasoning: non_empty(reasoning, place, "reasoning")?,
        })
    }
}

impl RawExplanation {
    fn into_example(self, place: &Place) -> Result<ExplanationExample, LabelError> {
        let Self {
            id,
            by,
            on,
            note,
            labels,
            candidate,
            paragraphs,
        } = self;
        Ok(ExplanationExample {
            example: RawCommon::new(id, by, on, note, labels).into_example(place)?,
            candidate: ExampleId(non_empty(candidate, place, "candidate")?),
            paragraphs: paragraphs
                .into_iter()
                .map(RawParagraph::into_paragraph)
                .collect(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParagraph {
    criterion: usize,
    outcome: RawOutcome,
    text: String,
    #[serde(default)]
    citations: Vec<RawCitation>,
}

impl RawParagraph {
    fn into_paragraph(self) -> ExampleParagraph {
        ExampleParagraph {
            criterion: self.criterion,
            outcome: match self.outcome {
                RawOutcome::NotMet => CriterionOutcome::NotMet,
                RawOutcome::NeedsReview => CriterionOutcome::NeedsReview,
            },
            text: self.text,
            citations: self
                .citations
                .into_iter()
                .map(RawCitation::into_written)
                .collect(),
        }
    }
}

/// The outcomes an explanation is written for.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawOutcome {
    NotMet,
    NeedsReview,
}

/// A label as written: `"yes"` or `"no"`, or a table that also names who
/// set it and when.
#[derive(Debug)]
enum RawLabel {
    Plain(Expected),
    Own(OwnLabel),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnLabel {
    label: Expected,
    by: String,
    on: Datetime,
}

impl<'de> Deserialize<'de> for RawLabel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LabelVisitor;

        impl<'de> Visitor<'de> for LabelVisitor {
            type Value = RawLabel;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("\"yes\" or \"no\", or a table with `label`, `by` and `on`")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<RawLabel, E> {
                Expected::deserialize(value.into_deserializer()).map(RawLabel::Plain)
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RawLabel, A::Error> {
                OwnLabel::deserialize(de::value::MapAccessDeserializer::new(map)).map(RawLabel::Own)
            }
        }

        deserializer.deserialize_any(LabelVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Labels, LabelError> {
        Labels::parse([("seeds.toml", text)])
    }

    fn date(year: u16, month: u8, day: u8) -> LabelDate {
        LabelDate { year, month, day }
    }

    const GATE: &str = r#"
[[gate]]
id = "gate-retry-identity"
commit = "Preserve payment retry identity"
by = "Tomas"
on = 2026-10-22
note = "The change is about the payment's identity."

[gate.labels]
domain = "yes"
observability = "no"
"#;

    #[test]
    fn a_gate_example_reads_its_commit_and_a_label_per_area_with_who_made_it_and_when() {
        let labels = parse(GATE).unwrap();

        let [gate] = labels.gate() else {
            panic!("expected one gate example: {labels:?}");
        };
        assert_eq!(gate.example().id().as_str(), "gate-retry-identity");
        assert_eq!(gate.commit(), "Preserve payment retry identity");
        assert_eq!(
            gate.example().note(),
            Some("The change is about the payment's identity.")
        );
        let made = |expected| Label {
            expected,
            by: Labeller::Person("Tomas".to_owned()),
            on: date(2026, 10, 22),
        };
        assert_eq!(
            gate.example().labels(),
            [
                ("domain".to_owned(), made(Expected::Yes)),
                ("observability".to_owned(), made(Expected::No)),
            ]
        );
        assert_eq!(gate.example().place().to_string(), "seeds.toml, line 2");
    }

    #[test]
    fn a_label_by_agent_is_a_seed_and_a_label_may_name_its_own_labeller_and_day() {
        let labels = parse(
            r#"
[[gate]]
id = "gate-rename"
commit = "Rename ChargeOutcome to ProviderOutcome"
by = "agent"
on = 2026-10-21
labels = { domain = "no", scalability = { label = "no", by = "Tomas", on = 2026-10-23 } }
"#,
        )
        .unwrap();

        let labels = labels.gate()[0].example().labels();
        assert_eq!(labels[0].1.by(), &Labeller::Agent);
        assert_eq!(labels[0].1.on().to_string(), "2026-10-21");
        assert_eq!(labels[1].1.by(), &Labeller::Person("Tomas".to_owned()));
        assert_eq!(labels[1].1.on().to_string(), "2026-10-23");
    }

    #[test]
    fn a_label_that_is_neither_yes_nor_no_names_its_line_and_what_it_must_be() {
        let error = parse(&GATE.replace(r#"observability = "no""#, r#"observability = "maybe""#))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "seeds.toml, line 11: unknown variant `maybe`, expected `yes` or `no`"
        );
    }

    #[test]
    fn a_label_table_without_its_labeller_names_what_it_lacks() {
        let error = parse(&GATE.replace(
            r#"observability = "no""#,
            r#"observability = { label = "no", on = 2026-10-23 }"#,
        ))
        .unwrap_err();

        assert_eq!(error.to_string(), "seeds.toml, line 11: missing field `by`");
    }

    #[test]
    fn a_key_the_format_does_not_have_is_refused() {
        let error = parse(&GATE.replace("note =", "notes =")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "seeds.toml, line 7: unknown field `notes`, expected one of `id`, `by`, `on`, \
             `note`, `labels`, `commit`"
        );
    }

    #[test]
    fn a_candidate_s_area_is_an_area_id() {
        let error =
            parse(&crate::samples::CANDIDATE.replace("area = \"domain\"", "area = \"Data model\""))
                .unwrap_err();

        assert_eq!(
            error.to_string(),
            "seeds.toml, line 2: the candidate's `area` is not valid"
        );
        assert_eq!(
            std::error::Error::source(&error).map(ToString::to_string),
            Some(
                "\"Data model\" is not an area ID: use lowercase letters, digits and hyphens"
                    .to_owned()
            )
        );
    }

    #[test]
    fn on_must_be_a_date() {
        let error =
            parse(&GATE.replace("on = 2026-10-22", "on = 2026-10-22T10:00:00")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "seeds.toml, line 2: `on` must be a date, such as 2026-10-21"
        );
    }

    #[test]
    fn an_example_needs_labels_and_a_labeller() {
        let no_labels = GATE.replace("domain = \"yes\"\nobservability = \"no\"\n", "");
        assert_eq!(
            parse(&no_labels).unwrap_err().to_string(),
            "seeds.toml, line 2: `gate-retry-identity` has no labels"
        );
        assert_eq!(
            parse(&GATE.replace(r#"by = "Tomas""#, r#"by = " ""#))
                .unwrap_err()
                .to_string(),
            "seeds.toml, line 2: `by` is empty"
        );
    }

    #[test]
    fn an_id_is_unique_across_files() {
        let error = Labels::parse([("gate.toml", GATE), ("more.toml", GATE)]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "more.toml, line 2: `gate-retry-identity` is already the ID of the example at \
             gate.toml, line 2"
        );
    }

    #[test]
    fn a_candidate_reads_as_the_generator_would_write_it() {
        let labels = parse(
            r#"
[[candidate]]
id = "retry-key"
commit = "Preserve payment retry identity"
area = "domain"
by = "agent"
on = 2026-10-21
stem = "Why does every attempt now carry the same key?"
answer_key = "A"
why_it_matters = "A retry must never charge twice."
hint = "Read ChargeRequest::new."
options = [
  { letter = "A", text = "So the provider charges once.", rationale = "Right." },
  { letter = "B", text = "So it is faster.", rationale = "Wrong." },
  { letter = "C", text = "So it is logged.", rationale = "Wrong." },
]
rubric = [{ statement = "Names the key.", met = "Says the key.", not_met = "Doesn't." }]
citations = [{ path = "src/provider.rs", first_line = 26, last_line = 37 }]
labels = { option_a = "yes" }
"#,
        )
        .unwrap();

        let [candidate] = labels.candidates() else {
            panic!("expected one candidate: {labels:?}");
        };
        assert_eq!(candidate.area().as_str(), "domain");
        let written = candidate.written();
        assert_eq!(written.question_type, QuestionType::SingleChoice);
        assert_eq!(written.options[1].letter, "B");
        assert_eq!(written.answer_key, "A");
        assert_eq!(written.rubric[0].not_met, "Doesn't.");
        assert_eq!(
            written.citations,
            [WrittenCitation {
                path: "src/provider.rs".to_owned(),
                first_line: 26,
                last_line: 37,
            }]
        );
    }

    #[test]
    fn a_learner_vote_reads_its_source_and_the_commit_of_the_learner_s_own_repository() {
        let showcase = parse(&crate::samples::vote("", "answer_probe = \"yes\"\n")).unwrap();
        let [vote] = showcase.candidates() else {
            panic!("expected one candidate: {showcase:?}");
        };
        assert_eq!(vote.source(), Source::LearnerVote);
        assert_eq!(vote.commit_id(), None);
        assert_eq!(
            vote.example().labels()[0].1.by(),
            &Labeller::Person("learner".to_owned())
        );

        let own = parse(&crate::samples::vote(
            "commit_id = \"0123456789abcdef0123456789abcdef01234567\"\n",
            "answer_probe = \"yes\"\n",
        ))
        .unwrap();
        assert_eq!(
            own.candidates()[0].commit_id().map(CommitId::full),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        let short = parse(&crate::samples::vote(
            "commit_id = \"0123456\"\n",
            "answer_probe = \"yes\"\n",
        ))
        .unwrap_err();
        assert_eq!(
            short.to_string(),
            "seeds.toml, line 2: the vote's `commit_id` is not a commit's full ID"
        );

        let labelled = parse(crate::samples::CANDIDATE).unwrap();
        assert_eq!(labelled.candidates()[0].source(), Source::Labelled);
    }

    #[test]
    fn only_a_learner_vote_is_about_a_commit_of_the_learner_s_own_repository() {
        let error = parse(&crate::samples::CANDIDATE.replace(
            "area = \"domain\"\n",
            "area = \"domain\"\ncommit_id = \"0123456789abcdef\"\n",
        ))
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "seeds.toml, line 2: `commit_id` names a commit of a learner's own repository, which \
             only a learner vote (`source = \"learner vote\"`) is about"
        );
        let unknown = crate::samples::vote("", "answer_probe = \"yes\"\n")
            .replace("source = \"learner vote\"", "source = \"agent\"");
        assert_eq!(
            parse(&unknown).unwrap_err().to_string(),
            "seeds.toml, line 4: unknown variant `agent`, expected `learner vote`"
        );
    }

    #[test]
    fn an_explanation_is_written_only_for_a_criterion_not_met_or_needing_review() {
        let explanation = |outcome: &str| {
            format!(
                r#"
[[explanation]]
id = "retry-key-explained"
candidate = "retry-key"
by = "agent"
on = 2026-10-21
paragraphs = [{{ criterion = 1, outcome = "{outcome}", text = "It misses the key." }}]
labels = {{ criterion_1_supported = "yes", criterion_1_agrees = "yes" }}
"#
            )
        };

        let labels = parse(&explanation("needs_review")).unwrap();
        let paragraph = &labels.explanations()[0].paragraphs()[0];
        assert_eq!(paragraph.criterion, 1);
        assert_eq!(paragraph.outcome, CriterionOutcome::NeedsReview);
        assert_eq!(paragraph.citations, []);
        assert_eq!(
            parse(&explanation("met")).unwrap_err().to_string(),
            "seeds.toml, line 7: unknown variant `met`, expected `not_met` or `needs_review`"
        );
    }
}
