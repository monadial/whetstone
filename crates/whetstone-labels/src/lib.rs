//! A maintainer's tool that measures Jev against the showcase project's
//! labelled examples, so that the assessment policy's cut-offs can be set
//! from evidence rather than guessed.
//!
//! [`run`] is the whole program: `main.rs` only passes in the process's
//! arguments and [`Environment`]. It has two commands:
//!
//! - `check` reads the examples in `showcase/labels/` ([`Labels::parse`]) and
//!   checks them against the showcase project ([`Labels::prepare`]),
//!   offline and free;
//! - `measure` also sends each example's Jev call three times
//!   ([`measure()`]), and prints the [`Report`] as Markdown.
//!
//! Jev is called exactly as Practice calls it, with the AI crate's request
//! builders and strict decoders. The tool only reads the assessment policy:
//! setting its values is a recorded decision that bumps its version (rule
//! 38). It prints labels by ID, never an example's text, and logs nothing.

mod labels;
mod measure;
mod prepare;
mod report;
#[cfg(test)]
mod samples;

pub use crate::labels::{
    CandidateExample, Example, ExampleId, ExampleParagraph, Expected, ExplanationExample,
    GateExample, Label, LabelDate, LabelError, Labeller, Labels, Place, ResponseExample, Source,
};
pub use crate::measure::measure;
pub use crate::prepare::{Call, Item, ParagraphDrop, PrepareError, Prepared, SkipReason, Skipped};
pub use crate::report::{
    CutOffs, Failure, JobReport, Measured, Measurements, Report, Row, SWEEP, Spread,
};
/// The injected environment the key is read from (rule 55): the app's own.
pub use whetstone::Environment;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whetstone_ai::{AiError, OpenRouter, SecretString};
use whetstone_core::{Job, ModelId};

/// How many times `measure` sends each example's call.
pub const RUNS: NonZeroUsize = NonZeroUsize::new(3).expect("3 is not 0");

/// The Jev model the examples are measured with unless `--model` names
/// another: the app's default, which the assessment policy is checked
/// against.
pub const DEFAULT_JEV_MODEL: &str = "typesafe/jev-1.14";

/// The examples' folder, beside the showcase project in this checkout.
const LABELS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../showcase/labels");

/// Measures Jev against the showcase project's labelled examples.
///
/// `check` reads and checks the examples, offline. `measure` also sends each
/// example's Jev call three times, through `OpenRouter` with the key in
/// `OPENROUTER_API_KEY`, and prints how often Jev's clear answers disagree
/// with the labels at candidate cut-offs, how many answers are unclear, and
/// how much answers spread across runs. Neither changes the assessment
/// policy.
#[derive(Debug, Parser)]
#[command(name = "whetstone-labels", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read the labelled examples and check them against the showcase
    /// project, without calling Jev.
    Check {
        /// The folder of labelled examples, or one file of them, such as a
        /// learner's labels file of votes [default: showcase/labels]
        #[arg(long)]
        labels: Option<PathBuf>,
    },
    /// Send each labelled example's Jev call three times and print the
    /// report as Markdown. Needs `OPENROUTER_API_KEY`, and costs credit.
    Measure {
        /// The folder of labelled examples, or one file of them, such as a
        /// learner's labels file of votes [default: showcase/labels]
        #[arg(long)]
        labels: Option<PathBuf>,
        /// The Jev model to measure.
        #[arg(long, default_value = DEFAULT_JEV_MODEL)]
        model: String,
    },
}

/// Why the tool stopped.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The examples' folder or one of its files can't be read.
    #[error("cannot read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The examples' folder holds no `.toml` file.
    #[error("{} holds no labelled examples (`.toml` files)", dir.display())]
    NoFiles { dir: PathBuf },
    /// A file isn't in the format.
    #[error(transparent)]
    Labels(#[from] LabelError),
    /// An example doesn't fit the showcase project.
    #[error(transparent)]
    Prepare(#[from] PrepareError),
    /// `measure` needs the key, and it isn't set.
    #[error(
        "no OpenRouter key: OPENROUTER_API_KEY is not set, and `measure` calls Jev through OpenRouter"
    )]
    NoKey,
    /// The key isn't valid Unicode.
    #[error("the OpenRouter key in OPENROUTER_API_KEY is not valid Unicode")]
    KeyNotUnicode,
    /// The connection couldn't be made.
    #[error("cannot call Jev")]
    Connection(#[source] AiError),
    /// The runtime the calls run on couldn't start.
    #[error("cannot start the runtime the calls run on")]
    Runtime(#[source] io::Error),
}

/// Runs the tool with the given command-line arguments, the first of which
/// is the program name, in the given environment: the report or summary on
/// stdout, an error on stderr.
pub fn run<I, T>(args: I, environment: &Environment) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(outcome) => {
            // Nothing useful remains to report if the stream itself is gone.
            drop(outcome.print());
            return ExitCode::from(u8::try_from(outcome.exit_code()).unwrap_or(1));
        }
    };
    match execute(cli.command, environment) {
        Ok(output) => {
            #[expect(
                clippy::print_stdout,
                reason = "the tool's output is its report or summary, on stdout"
            )]
            {
                print!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            #[expect(
                clippy::print_stderr,
                reason = "the tool reports why it stopped on stderr"
            )]
            {
                eprintln!("error: {}", with_causes(&error));
            }
            ExitCode::FAILURE
        }
    }
}

/// `error`'s message, followed by each of its causes', as `error: cause`.
fn with_causes(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        write!(message, ": {source}").expect("writing to a String never fails");
        cause = source.source();
    }
    message
}

/// The text `command` prints.
fn execute(command: Command, environment: &Environment) -> Result<String, ToolError> {
    let showcase = whetstone::embedded_showcase();
    match command {
        Command::Check { labels } => {
            let dir = labels_dir(labels.as_deref(), environment);
            let model = ModelId::new(DEFAULT_JEV_MODEL.to_owned());
            let prepared = load(&dir)?.prepare(&showcase, &model)?;
            Ok(summary(&prepared))
        }
        Command::Measure { labels, model } => {
            // The key is read here and only here, from the environment
            // `main.rs` captured, and goes straight into the connection,
            // which exposes it only where the auth header is built (rule 41).
            let key = environment
                .get("OPENROUTER_API_KEY")
                .ok_or(ToolError::NoKey)?
                .to_str()
                .ok_or(ToolError::KeyNotUnicode)?;
            let connection = OpenRouter::new(Some(SecretString::from(key.to_owned())))
                .map_err(ToolError::Connection)?;
            let dir = labels_dir(labels.as_deref(), environment);
            let prepared = load(&dir)?.prepare(&showcase, &ModelId::new(model))?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(ToolError::Runtime)?;
            let measurements = runtime.block_on(measure(&connection, &prepared, RUNS));
            Ok(Report::new(measurements).to_string())
        }
    }
}

/// The examples' folder: `labels`, against the directory the tool started
/// in, or the one beside the showcase project.
fn labels_dir(labels: Option<&Path>, environment: &Environment) -> PathBuf {
    labels.map_or_else(
        || PathBuf::from(LABELS),
        |labels| environment.current_dir().join(labels),
    )
}

/// Reads every `.toml` file in the folder at `path`, in file-name order, or,
/// when `path` is a file, such as a learner's labels file of votes, that
/// file alone.
pub fn load(path: &Path) -> Result<Labels, ToolError> {
    let read = |path: &Path, source| ToolError::Read {
        path: path.to_owned(),
        source,
    };
    let mut paths = Vec::new();
    if path.is_file() {
        paths.push(path.to_owned());
    } else {
        for entry in fs::read_dir(path).map_err(|source| read(path, source))? {
            paths.push(entry.map_err(|source| read(path, source))?.path());
        }
    }
    let mut files = Vec::new();
    for file in paths {
        if file
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            let name = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = fs::read_to_string(&file).map_err(|source| read(&file, source))?;
            files.push((name, text));
        }
    }
    if files.is_empty() {
        return Err(ToolError::NoFiles {
            dir: path.to_owned(),
        });
    }
    files.sort();
    Ok(Labels::parse(
        files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
    )?)
}

/// What `check` prints: the examples' labels per job, and what measuring
/// them takes.
fn summary(prepared: &Prepared) -> String {
    let items: Vec<&Item> = prepared.items().collect();
    let mut summary = format!(
        "{} examples, {} labels, {} of them by an agent, as seeds still to review.\n",
        prepared.calls().len(),
        items.len(),
        prepared.by_agent(),
    );
    if let Some(not_measured) = report::not_measured(prepared.skipped()) {
        writeln!(summary, "Not measured: {not_measured}").expect("writing to a String never fails");
    }
    for job in Job::ALL {
        let labels = items.iter().filter(|item| item.job() == job).count();
        let yes = items
            .iter()
            .filter(|item| item.job() == job && item.label().expected() == Expected::Yes)
            .count();
        writeln!(
            summary,
            "- {}: {labels} labels ({yes} yes, {} no)",
            job_name(job),
            labels - yes
        )
        .expect("writing to a String never fails");
    }
    writeln!(
        summary,
        "Measuring them makes {} calls to Jev: {RUNS} runs of {}.",
        prepared.calls().len() * RUNS.get(),
        prepared.calls().len()
    )
    .expect("writing to a String never fails");
    summary
}

/// A job's name, as the summary and the report head its part.
pub(crate) fn job_name(job: Job) -> &'static str {
    match job {
        Job::RelevanceGate => "Relevance gate",
        Job::QuestionCheck => "Question check (checks 1 to 6)",
        Job::OptionRating => "Option rating",
        Job::RationaleAndHintChecks => "Rationale and hint checks",
        Job::ReasoningCriterion => "Reasoning criterion",
        Job::ExplanationCheck => "Explanation check",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_examples_fit_the_showcase_project_and_label_every_job() {
        let prepared = load(Path::new(LABELS))
            .unwrap()
            .prepare(
                &whetstone::embedded_showcase(),
                &ModelId::new(DEFAULT_JEV_MODEL.to_owned()),
            )
            .unwrap();

        for job in Job::ALL {
            assert!(
                prepared.items().any(|item| item.job() == job),
                "no seed labels {job:?}"
            );
        }
    }

    #[test]
    fn measuring_reads_the_key_from_the_injected_environment() {
        let measure = || Command::Measure {
            labels: None,
            model: DEFAULT_JEV_MODEL.to_owned(),
        };

        let unset = execute(measure(), &Environment::default()).unwrap_err();
        assert!(matches!(unset, ToolError::NoKey), "{unset}");

        let blank: Environment = [("OPENROUTER_API_KEY", " ")].into_iter().collect();
        let blank = execute(measure(), &blank).unwrap_err();
        assert!(
            matches!(blank, ToolError::Connection(AiError::NoKey)),
            "{blank}"
        );
    }

    #[test]
    fn why_the_tool_stopped_names_each_cause() {
        let unread = ToolError::Read {
            path: PathBuf::from("showcase/labels"),
            source: io::Error::new(io::ErrorKind::NotFound, "no such directory"),
        };
        assert_eq!(
            with_causes(&unread),
            "cannot read showcase/labels: no such directory"
        );

        assert_eq!(
            with_causes(&ToolError::Connection(AiError::NoKey)),
            "cannot call Jev: no OpenRouter key: OPENROUTER_API_KEY is not set"
        );
    }

    #[test]
    fn labels_are_read_from_one_file_as_well_as_from_a_folder() {
        let file = load(&Path::new(LABELS).join("gate.toml")).unwrap();

        assert_ne!(file.gate(), []);
        assert_eq!(file.candidates(), []);
    }

    #[test]
    fn check_counts_the_learner_votes_it_cannot_measure() {
        let votes = samples::vote(
            "commit_id = \"0123456789abcdef0123456789abcdef01234567\"\n",
            "no_giveaway = \"yes\"\n",
        );
        let prepared = samples::prepared(&format!("{}{votes}", samples::CANDIDATE)).unwrap();

        assert_eq!(
            summary(&prepared).lines().nth(1),
            Some(
                "Not measured: 1 learner vote, which the tool can't send to Jev as Practice sent \
                 them: 1 about a commit of the learner's own repository, whose code would go out \
                 again without a new outbound review."
            )
        );
    }

    /// A learner's labels file as Whetstone's review of withheld questions
    /// writes it: the snapshot its app-seam test keeps.
    const VOTES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../whetstone/tests/it/snapshots/it__review_withheld__labels_file.snap"
    );

    #[test]
    fn a_learner_s_labels_file_as_whetstone_writes_it_is_read_and_measured() {
        let snapshot = fs::read_to_string(VOTES).unwrap();
        let (_, written) = snapshot
            .split_once("\n---\n")
            .expect("the snapshot's header ends");

        let prepared = Labels::parse([("labels.toml", written)])
            .unwrap()
            .prepare(
                &whetstone::embedded_showcase(),
                &ModelId::new(DEFAULT_JEV_MODEL.to_owned()),
            )
            .unwrap();

        let names: Vec<&str> = prepared.items().map(Item::name).collect();
        assert_eq!(names, ["learner-vote-20261021T101502-1/no_giveaway"]);
        assert_eq!(prepared.skipped(), []);
        assert_eq!(
            prepared
                .items()
                .next()
                .map(|item| item.label().by().to_string()),
            Some("learner".to_owned())
        );
    }

    #[test]
    fn a_folder_without_examples_says_so() {
        let error = load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))).unwrap_err();

        assert!(matches!(error, ToolError::NoFiles { .. }), "{error}");
    }
}
