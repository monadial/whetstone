//! What a keyed run found, per job: how often Jev's clear answers disagree
//! with the labels at candidate cut-offs, how many answers land in unclear,
//! and how much the answers to one label spread across runs.
//!
//! Every count is of answers: one per label per run that answered. A clear
//! answer disagrees when it is clear yes for a label of no, or clear no for
//! a label of yes. The report only reads the assessment policy: setting its
//! values is a recorded decision that bumps its version (rule 38).

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use whetstone_ai::{AiError, QuestionCheckCall};
use whetstone_core::{AssessmentPolicy, Band, Job, ModelId, Probability};

use crate::labels::{ExampleId, Expected};
use crate::prepare::{SkipReason, Skipped};

/// One label, and Jev's answer to it in each run that answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    /// Its example's ID and its key, as `id/key`.
    pub name: String,
    /// The job whose cut-offs band its answers.
    pub job: Job,
    /// What it says Jev should answer.
    pub expected: Expected,
    /// Jev's answers, one per run that answered, in run order.
    pub answers: Vec<Probability>,
}

/// A call that got no answers in one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The example whose call it was.
    pub example: ExampleId,
    /// Which call of a candidate example's question check it was; none for
    /// an example of one call.
    pub call: Option<QuestionCheckCall>,
    /// The run, counting from 1.
    pub run: usize,
    /// Why it failed.
    pub error: AiError,
}

/// Everything a keyed run measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurements {
    /// The model every call asked for.
    pub model: ModelId,
    /// The models the replies reported.
    pub reported_models: BTreeSet<String>,
    /// How many times each example's call was made.
    pub runs: usize,
    /// How many calls were made, failed ones included.
    pub calls: usize,
    /// The calls that got no answers.
    pub failures: Vec<Failure>,
    /// The cost the replies reported, in USD.
    pub cost: f64,
    /// How many replies reported no cost.
    pub cost_not_reported: usize,
    /// Every label, with its answers.
    pub labels: Vec<Measured>,
    /// How many labels an agent made, as seeds still to review.
    pub by_agent: usize,
    /// The learner votes not measured, each with why.
    pub skipped: Vec<Skipped>,
}

/// A pair of cut-offs a report row bands the answers with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CutOffs {
    /// A candidate pair: clear yes at or above `upper`, clear no at or
    /// below `lower`.
    Candidate {
        /// The upper cut-off.
        upper: f64,
        /// The lower cut-off.
        lower: f64,
    },
    /// The assessment policy in use, as it bands each job.
    Policy,
}

impl CutOffs {
    /// The band of `answer` to a check of `job`.
    #[must_use]
    pub fn band(self, job: Job, answer: Probability) -> Band {
        match self {
            Self::Candidate { upper, lower } => {
                if answer.get() >= upper {
                    Band::ClearYes
                } else if answer.get() <= lower {
                    Band::ClearNo
                } else {
                    Band::Unclear
                }
            }
            Self::Policy => AssessmentPolicy::current().band(job, answer),
        }
    }
}

/// The candidate cut-offs every job is measured at, from the loosest to the
/// strictest, around policy 0's 0.80 and 0.20. The clear-yes counts depend
/// only on the upper cut-off and the clear-no counts only on the lower, so
/// any pair of them can be read off these rows.
pub const SWEEP: [CutOffs; 5] = [
    CutOffs::Candidate {
        upper: 0.70,
        lower: 0.30,
    },
    CutOffs::Candidate {
        upper: 0.75,
        lower: 0.25,
    },
    CutOffs::Candidate {
        upper: 0.80,
        lower: 0.20,
    },
    CutOffs::Candidate {
        upper: 0.85,
        lower: 0.15,
    },
    CutOffs::Candidate {
        upper: 0.90,
        lower: 0.10,
    },
];

/// A job's answers banded at one pair of cut-offs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// The cut-offs.
    pub cut_offs: CutOffs,
    /// Answers that are clear yes.
    pub clear_yes: usize,
    /// Of those, the ones whose label is no.
    pub wrong_yes: usize,
    /// Answers that are clear no.
    pub clear_no: usize,
    /// Of those, the ones whose label is yes.
    pub wrong_no: usize,
    /// Answers between the cut-offs.
    pub unclear: usize,
}

impl Row {
    /// The share of clear answers that disagree with their labels, or
    /// `None` when no answer is clear.
    #[must_use]
    pub fn disagreement(&self) -> Option<f64> {
        let clear = self.clear_yes + self.clear_no;
        (clear > 0).then(|| ratio(self.wrong_yes + self.wrong_no, clear))
    }
}

/// How much the answers to one label vary across runs: for each label, its
/// largest answer minus its smallest.
#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    /// The mean over labels answered at least twice, or `None` when none
    /// was.
    pub mean: Option<f64>,
    /// The label that varies most, and by how much.
    pub largest: Option<(String, f64)>,
    /// How many labels' answers fall in more than one band under the
    /// assessment policy in use.
    pub band_changes: usize,
}

/// One job's part of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct JobReport {
    job: Job,
    yes: usize,
    no: usize,
    answers: usize,
    rows: Vec<Row>,
    spread: Spread,
    flagged: Vec<Measured>,
}

impl JobReport {
    /// The report on the labels in `labels` whose job is `job`: one row per
    /// pair of cut-offs in [`SWEEP`], then one for the assessment policy in
    /// use.
    #[must_use]
    pub fn new(job: Job, labels: &[Measured]) -> Self {
        let labels: Vec<&Measured> = labels.iter().filter(|label| label.job == job).collect();
        let rows = SWEEP
            .into_iter()
            .chain([CutOffs::Policy])
            .map(|cut_offs| row(job, cut_offs, &labels))
            .collect();
        let bands = |label: &Measured| -> BTreeSet<u8> {
            label
                .answers
                .iter()
                .map(|&answer| band_order(CutOffs::Policy.band(job, answer)))
                .collect()
        };
        let ranges: Vec<(&str, f64)> = labels
            .iter()
            .filter(|label| label.answers.len() >= 2)
            .map(|label| (label.name.as_str(), range(&label.answers)))
            .collect();
        let spread = Spread {
            mean: (!ranges.is_empty())
                .then(|| ranges.iter().map(|(_, range)| range).sum::<f64>() / as_f64(ranges.len())),
            largest: ranges
                .iter()
                .copied()
                .reduce(|largest, next| if next.1 > largest.1 { next } else { largest })
                .map(|(name, range)| (name.to_owned(), range)),
            band_changes: labels.iter().filter(|label| bands(label).len() > 1).count(),
        };
        let flagged = labels
            .iter()
            .filter(|label| {
                label.answers.iter().any(|&answer| {
                    !matches!(
                        (CutOffs::Policy.band(job, answer), label.expected),
                        (Band::ClearYes, Expected::Yes) | (Band::ClearNo, Expected::No)
                    )
                })
            })
            .map(|&label| label.clone())
            .collect();
        Self {
            job,
            yes: labels
                .iter()
                .filter(|label| label.expected == Expected::Yes)
                .count(),
            no: labels
                .iter()
                .filter(|label| label.expected == Expected::No)
                .count(),
            answers: labels.iter().map(|label| label.answers.len()).sum(),
            rows,
            spread,
            flagged,
        }
    }

    /// Its job.
    #[must_use]
    pub fn job(&self) -> Job {
        self.job
    }

    /// How many of its labels say yes.
    #[must_use]
    pub fn yes(&self) -> usize {
        self.yes
    }

    /// How many of its labels say no.
    #[must_use]
    pub fn no(&self) -> usize {
        self.no
    }

    /// How many answers it measured.
    #[must_use]
    pub fn answers(&self) -> usize {
        self.answers
    }

    /// One row per pair of cut-offs in [`SWEEP`], then the policy's.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// How much answers vary across runs.
    #[must_use]
    pub fn spread(&self) -> &Spread {
        &self.spread
    }

    /// The labels with an answer that is unclear or disagrees under the
    /// assessment policy in use, in label order.
    #[must_use]
    pub fn flagged(&self) -> &[Measured] {
        &self.flagged
    }
}

fn row(job: Job, cut_offs: CutOffs, labels: &[&Measured]) -> Row {
    let mut row = Row {
        cut_offs,
        clear_yes: 0,
        wrong_yes: 0,
        clear_no: 0,
        wrong_no: 0,
        unclear: 0,
    };
    for label in labels {
        for &answer in &label.answers {
            match (cut_offs.band(job, answer), label.expected) {
                (Band::ClearYes, expected) => {
                    row.clear_yes += 1;
                    row.wrong_yes += usize::from(expected == Expected::No);
                }
                (Band::ClearNo, expected) => {
                    row.clear_no += 1;
                    row.wrong_no += usize::from(expected == Expected::Yes);
                }
                (Band::Unclear, _) => row.unclear += 1,
            }
        }
    }
    row
}

fn band_order(band: Band) -> u8 {
    match band {
        Band::ClearNo => 0,
        Band::Unclear => 1,
        Band::ClearYes => 2,
    }
}

/// The largest of `answers` minus the smallest.
fn range(answers: &[Probability]) -> f64 {
    let values = answers.iter().map(|answer| answer.get());
    values.clone().fold(f64::MIN, f64::max) - values.fold(f64::MAX, f64::min)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a count of labels or answers is far below 2^52"
)]
fn as_f64(n: usize) -> f64 {
    n as f64
}

fn ratio(part: usize, whole: usize) -> f64 {
    as_f64(part) / as_f64(whole)
}

/// The whole report: the run, then each job in [`Job::ALL`] order.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    measurements: Measurements,
    jobs: Vec<JobReport>,
}

impl Report {
    /// The report on `measurements`.
    #[must_use]
    pub fn new(measurements: Measurements) -> Self {
        let jobs = Job::ALL
            .into_iter()
            .map(|job| JobReport::new(job, &measurements.labels))
            .collect();
        Self { measurements, jobs }
    }

    /// Each job's part, in [`Job::ALL`] order.
    #[must_use]
    pub fn jobs(&self) -> &[JobReport] {
        &self.jobs
    }
}

/// The report as Markdown, to paste into the labelling ticket. It names
/// labels by ID and shows Jev's probabilities, never an example's text.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let run = &self.measurements;
        let policy = AssessmentPolicy::current();
        writeln!(f, "# Jev against the labelled examples\n")?;
        let reported = if run.reported_models.is_empty() {
            "no model reported".to_owned()
        } else {
            let models: Vec<String> = run
                .reported_models
                .iter()
                .map(|model| format!("`{model}`"))
                .collect();
            format!("reported as {}", models.join(", "))
        };
        writeln!(f, "- **Model:** `{}`, {reported}.", run.model)?;
        writeln!(
            f,
            "- **Runs:** each example {} times: {} calls, {} failed.",
            run.runs,
            run.calls,
            run.failures.len()
        )?;
        writeln!(
            f,
            "- **Cost:** {:.6} USD, as reported{}.",
            run.cost,
            match run.cost_not_reported {
                0 => String::new(),
                1 => "; not reported for 1 call".to_owned(),
                n => format!("; not reported for {n} calls"),
            }
        )?;
        writeln!(
            f,
            "- **Labels:** {}, of which {} by an agent, as seeds still to review.",
            run.labels.len(),
            run.by_agent
        )?;
        if let Some(not_measured) = not_measured(&run.skipped) {
            writeln!(f, "- **Not measured:** {not_measured}")?;
        }
        writeln!(
            f,
            "- **Assessment policy:** version {}, {}. This tool never changes it: setting its \
             values is a recorded decision that bumps its version (rule 38).",
            policy.version(),
            match policy.validation() {
                whetstone_core::Validation::Unvalidated => "unvalidated",
                whetstone_core::Validation::Validated => "validated",
            }
        )?;
        writeln!(
            f,
            "\nCounts are of answers, one per label per run. A clear answer disagrees when it is \
             clear yes for a label of no, or clear no for a label of yes. The clear-yes columns \
             depend only on the upper cut-off and the clear-no columns only on the lower, so any \
             pair can be read off the rows."
        )?;
        if !run.failures.is_empty() {
            writeln!(f, "\n## Failed calls\n")?;
            for failure in &run.failures {
                let call = failure
                    .call
                    .map(|call| format!(" ({} call)", call_name(call)))
                    .unwrap_or_default();
                writeln!(
                    f,
                    "- `{}`{call}, run {}: {}",
                    failure.example, failure.run, failure.error
                )?;
            }
        }
        for job in &self.jobs {
            write_job(f, job)?;
        }
        Ok(())
    }
}

/// A question check call's name, as the report names a failed one.
fn call_name(call: QuestionCheckCall) -> &'static str {
    match call {
        QuestionCheckCall::OptionRatings => "option ratings",
        QuestionCheckCall::Hint => "hint",
        QuestionCheckCall::Candidate => "candidate",
    }
}

fn write_job(f: &mut fmt::Formatter<'_>, job: &JobReport) -> fmt::Result {
    writeln!(f, "\n## {}\n", crate::job_name(job.job))?;
    if job.yes + job.no == 0 {
        return writeln!(f, "No labels yet.");
    }
    writeln!(
        f,
        "{} ({} yes, {} no), {}.\n",
        counted(job.yes + job.no, "label", "labels"),
        job.yes,
        job.no,
        counted(job.answers, "answer", "answers")
    )?;
    writeln!(
        f,
        "| Cut-offs | Clear yes | … labelled no | Clear no | … labelled yes | Unclear | Clear \
         answers that disagree |"
    )?;
    writeln!(f, "| --- | ---: | ---: | ---: | ---: | ---: | ---: |")?;
    for row in &job.rows {
        let cut_offs = match row.cut_offs {
            CutOffs::Candidate { upper, lower } => format!("{upper:.2} / {lower:.2}"),
            CutOffs::Policy => format!("policy {} (in use)", AssessmentPolicy::current().version()),
        };
        let disagreement = row.disagreement().map_or_else(
            || "no clear answers".to_owned(),
            |share| {
                format!(
                    "{} of {} ({:.0}%)",
                    row.wrong_yes + row.wrong_no,
                    row.clear_yes + row.clear_no,
                    share * 100.0
                )
            },
        );
        writeln!(
            f,
            "| {cut_offs} | {} | {} | {} | {} | {} | {disagreement} |",
            row.clear_yes, row.wrong_yes, row.clear_no, row.wrong_no, row.unclear
        )?;
    }
    let spread = &job.spread;
    match (spread.mean, &spread.largest) {
        (Some(mean), Some((name, largest))) => writeln!(
            f,
            "\nSpread across runs, each label's largest answer minus its smallest: mean {mean:.2}, \
             largest {largest:.2} (`{name}`). Under the policy in use, {} answers fall in more \
             than one band.",
            counted(spread.band_changes, "label's", "labels'")
        )?,
        _ => writeln!(f, "\nNo label was answered twice, so there is no spread.")?,
    }
    if !job.flagged.is_empty() {
        writeln!(
            f,
            "\nUnder the policy in use, these labels have answers that are unclear or \
             disagree:\n"
        )?;
        writeln!(f, "| Label | Labelled | Answers |")?;
        writeln!(f, "| --- | --- | --- |")?;
        for label in &job.flagged {
            let mut answers = String::new();
            for (index, answer) in label.answers.iter().enumerate() {
                if index > 0 {
                    answers.push_str(", ");
                }
                write!(answers, "{:.2}", answer.get())?;
            }
            writeln!(f, "| `{}` | {} | {answers} |", label.name, label.expected)?;
        }
    }
    Ok(())
}

/// `n` and the noun, as `one` when `n` is 1 and `many` otherwise.
fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What the tool says about the learner votes it `skipped`, if any: how
/// many, and why, as one sentence.
pub(crate) fn not_measured(skipped: &[Skipped]) -> Option<String> {
    if skipped.is_empty() {
        return None;
    }
    let reasons: Vec<String> = SkipReason::ALL
        .into_iter()
        .filter_map(
            |reason| match skipped.iter().filter(|vote| vote.reason == reason).count() {
                0 => None,
                n => Some(format!("{n} {reason}")),
            },
        )
        .collect();
    Some(format!(
        "{}, which the tool can't send to Jev as Practice sent them: {}.",
        counted(skipped.len(), "learner vote", "learner votes"),
        reasons.join(", and ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(name: &str, expected: Expected, answers: &[f64]) -> Measured {
        Measured {
            name: name.to_owned(),
            job: Job::OptionRating,
            expected,
            answers: answers
                .iter()
                .map(|&answer| Probability::new(answer).unwrap())
                .collect(),
        }
    }

    /// Option ratings worked through by hand in the tests below.
    fn option_ratings() -> Vec<Measured> {
        vec![
            measured("a/option_a", Expected::Yes, &[0.95, 0.72, 0.78]),
            measured("a/option_b", Expected::No, &[0.10, 0.25, 0.82]),
            measured("a/option_c", Expected::No, &[0.30, 0.29, 0.31]),
            measured("b/option_a", Expected::Yes, &[0.05, 0.15, 0.50]),
            measured("c/option_b", Expected::No, &[0.01, 0.02, 0.01]),
        ]
    }

    fn counts(row: &Row) -> [usize; 5] {
        [
            row.clear_yes,
            row.wrong_yes,
            row.clear_no,
            row.wrong_no,
            row.unclear,
        ]
    }

    #[test]
    fn clear_answers_disagree_with_their_labels_at_each_cut_off() {
        let report = JobReport::new(Job::OptionRating, &option_ratings());

        let rows: Vec<[usize; 5]> = report.rows().iter().map(counts).collect();
        // Clear yes, of them labelled no, clear no, of them labelled yes,
        // unclear.
        assert_eq!(
            rows,
            [
                [4, 1, 9, 2, 2], // 0.70 / 0.30
                [3, 1, 7, 2, 5], // 0.75 / 0.25
                [2, 1, 6, 2, 7], // 0.80 / 0.20
                [1, 0, 6, 2, 8], // 0.85 / 0.15
                [1, 0, 5, 1, 9], // 0.90 / 0.10
                [2, 1, 6, 2, 7], // policy 0, which is 0.80 / 0.20
            ]
        );
        let disagreement = |row: usize| report.rows()[row].disagreement().unwrap();
        assert!((disagreement(0) - 3.0 / 13.0).abs() < 1e-12);
        assert!((disagreement(2) - 3.0 / 8.0).abs() < 1e-12);
        assert!((disagreement(4) - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(report.rows()[5].cut_offs, CutOffs::Policy);
    }

    #[test]
    fn a_job_counts_its_labels_and_answers_and_no_one_else_s() {
        let mut labels = option_ratings();
        labels.push(Measured {
            job: Job::RelevanceGate,
            ..measured("gate/domain", Expected::Yes, &[0.9, 0.9, 0.9])
        });
        // A label whose second run failed.
        labels.push(measured("d/option_c", Expected::No, &[0.05, 0.06]));

        let report = JobReport::new(Job::OptionRating, &labels);

        assert_eq!((report.yes(), report.no(), report.answers()), (2, 4, 17));
    }

    #[test]
    fn the_spread_is_each_label_s_largest_answer_minus_its_smallest() {
        let report = JobReport::new(Job::OptionRating, &option_ratings());

        let spread = report.spread();
        // 0.23, 0.72, 0.02, 0.45 and 0.01.
        assert!((spread.mean.unwrap() - 1.43 / 5.0).abs() < 1e-9);
        let (name, largest) = spread.largest.clone().unwrap();
        assert_eq!(name, "a/option_b");
        assert!((largest - 0.72).abs() < 1e-9);
        // a/option_a is clear yes, then unclear; a/option_b clear no, unclear
        // and clear yes; b/option_a clear no, then unclear.
        assert_eq!(spread.band_changes, 3);
    }

    #[test]
    fn a_label_answered_once_has_no_spread() {
        let once = [measured("a/option_a", Expected::Yes, &[0.9])];

        let report = JobReport::new(Job::OptionRating, &once);

        assert_eq!(
            report.spread(),
            &Spread {
                mean: None,
                largest: None,
                band_changes: 0,
            }
        );
    }

    #[test]
    fn labels_with_an_unclear_or_disagreeing_answer_under_the_policy_are_flagged() {
        let report = JobReport::new(Job::OptionRating, &option_ratings());

        let flagged: Vec<&str> = report
            .flagged()
            .iter()
            .map(|label| label.name.as_str())
            .collect();
        assert_eq!(
            flagged,
            ["a/option_a", "a/option_b", "a/option_c", "b/option_a"]
        );
    }

    #[test]
    fn with_no_clear_answer_there_is_no_disagreement() {
        let unclear = [measured("a/option_a", Expected::Yes, &[0.5, 0.5])];

        let report = JobReport::new(Job::OptionRating, &unclear);

        assert_eq!(report.rows()[0].disagreement(), None);
    }

    #[test]
    fn the_report_reads_as_markdown_with_every_job() {
        let measurements = Measurements {
            model: ModelId::new("typesafe/jev-1.14".to_owned()),
            reported_models: BTreeSet::from(["typesafe/jev-1.14-20261014".to_owned()]),
            runs: 3,
            calls: 6,
            failures: vec![
                Failure {
                    example: ExampleId::new("a"),
                    call: Some(QuestionCheckCall::OptionRatings),
                    run: 1,
                    error: AiError::TimedOut,
                },
                Failure {
                    example: ExampleId::new("b"),
                    call: None,
                    run: 2,
                    error: AiError::RateLimited,
                },
            ],
            cost: 0.000_412,
            cost_not_reported: 1,
            labels: option_ratings(),
            by_agent: 5,
            skipped: Vec::new(),
        };

        insta::assert_snapshot!(Report::new(measurements).to_string());
    }

    #[test]
    fn the_report_counts_the_learner_votes_it_did_not_measure_and_says_why() {
        let skipped = |id: &str, reason| Skipped {
            example: ExampleId::new(id),
            reason,
        };
        let measurements = Measurements {
            model: ModelId::new("typesafe/jev-1.14".to_owned()),
            reported_models: BTreeSet::new(),
            runs: 3,
            calls: 0,
            failures: Vec::new(),
            cost: 0.0,
            cost_not_reported: 0,
            labels: Vec::new(),
            by_agent: 0,
            skipped: vec![
                skipped("vote-1", SkipReason::OwnRepository),
                skipped("vote-2", SkipReason::OwnRepository),
                skipped("vote-3", SkipReason::NotInEvidence),
            ],
        };

        let report = Report::new(measurements).to_string();

        assert!(
            report.contains(
                "- **Not measured:** 3 learner votes, which the tool can't send to Jev as \
                 Practice sent them: 2 about a commit of the learner's own repository, whose \
                 code would go out again without a new outbound review, and 1 citing lines \
                 the tool's evidence doesn't send, such as connected code.\n"
            ),
            "{report}"
        );
    }
}
