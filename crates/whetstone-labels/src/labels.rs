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
//! [`Labels::parse`] reads the files and checks what each says on its own.
//! Whether an example fits the showcase project, such as its commit, its
//! area, its citations and which labels it needs, is checked when it is
//! prepared ([`Labels::prepare`](crate::Labels::prepare)).

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, IntoDeserializer, MapAccess, Visitor};
use toml::Spanned;
use toml::value::Datetime;
use whetstone_core::{
    CriterionOutcome, QuestionType, WrittenCandidate, WrittenCitation, WrittenCriterion,
    WrittenOption,
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

/// What every example has: where it is, its ID, its note and its labels,
/// by key, in key order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    place: Place,
    id: String,
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
    pub fn id(&self) -> &str {
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
    commit: String,
    area: String,
    written: WrittenCandidate,
}

impl CandidateExample {
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

    /// The ID of the area it is about.
    #[must_use]
    pub fn area(&self) -> &str {
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
    candidate: String,
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
    pub fn candidate(&self) -> &str {
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
    candidate: String,
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
    pub fn candidate(&self) -> &str {
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
    NoLabels { place: Place, id: String },
    /// Two examples have the same ID.
    #[error("{second}: `{id}` is already the ID of the example at {first}")]
    Duplicate {
        id: String,
        first: Place,
        second: Place,
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
        let mut ids: BTreeMap<&str, &Place> = BTreeMap::new();
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
#[derive(Deserialize)]
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
        let id = non_empty(self.id, place, "id")?;
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGate {
    id: String,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
    commit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCandidate {
    id: String,
    by: String,
    on: Datetime,
    note: Option<String>,
    labels: BTreeMap<String, RawLabel>,
    commit: String,
    area: String,
    stem: String,
    options: Vec<RawOption>,
    answer_key: String,
    why_it_matters: String,
    rubric: Vec<RawCriterion>,
    hint: String,
    citations: Vec<RawCitation>,
}

/// A candidate's parts, as the generator writes them.
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
            by,
            on,
            note,
            labels,
            commit,
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
        Ok(CandidateExample {
            example: RawCommon::new(id, by, on, note, labels).into_example(place)?,
            commit: non_empty(commit, place, "commit")?,
            area: non_empty(area, place, "area")?,
            written: question.into_written(),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOption {
    letter: String,
    text: String,
    rationale: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCriterion {
    statement: String,
    met: String,
    not_met: String,
}

#[derive(Deserialize)]
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

#[derive(Deserialize)]
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

#[derive(Deserialize)]
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
            candidate: non_empty(candidate, place, "candidate")?,
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
            candidate: non_empty(candidate, place, "candidate")?,
            paragraphs: paragraphs
                .into_iter()
                .map(RawParagraph::into_paragraph)
                .collect(),
        })
    }
}

#[derive(Deserialize)]
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
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawOutcome {
    NotMet,
    NeedsReview,
}

/// A label as written: `"yes"` or `"no"`, or a table that also names who
/// set it and when.
enum RawLabel {
    Plain(Expected),
    Own(OwnLabel),
}

#[derive(Deserialize)]
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
        assert_eq!(gate.example().id(), "gate-retry-identity");
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
        assert_eq!(candidate.area(), "domain");
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
