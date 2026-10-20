//! A maintainer's tool that measures Jev against the showcase project's
//! labelled examples, so that the assessment policy's cut-offs can be set
//! from evidence rather than guessed.
//!
//! [`run`] is the whole program: `main.rs` only passes in the process's
//! arguments and [`Environment`]. Its `check` command reads the examples in
//! `showcase/labels/` ([`Labels::parse`]) and checks them against the
//! showcase project ([`Labels::prepare`]), offline and free, as the Jev
//! calls Practice would make for them, built with the AI crate's request
//! builders. The tool only reads the assessment policy: setting its values
//! is a recorded decision that bumps its version (rule 38). It prints
//! labels by ID, never an example's text, and logs nothing.

mod labels;
mod prepare;
#[cfg(test)]
mod samples;

pub use crate::labels::{
    CandidateExample, Example, ExampleParagraph, Expected, ExplanationExample, GateExample, Label,
    LabelDate, LabelError, Labeller, Labels, Place, ResponseExample,
};
pub use crate::prepare::{Call, Item, PrepareError, Prepared};
/// The injected environment the tool runs in (rule 55): the app's own.
pub use whetstone::Environment;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whetstone_core::{Job, ModelId};

/// How many times each example's call is sent to measure it.
pub const RUNS: NonZeroUsize = NonZeroUsize::new(3).expect("3 is not 0");

/// The Jev model the examples' calls ask: the app's default, which the
/// assessment policy is checked against.
pub const DEFAULT_JEV_MODEL: &str = "typesafe/jev-1.14";

/// The examples' folder, beside the showcase project in this checkout.
const LABELS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../showcase/labels");

/// Measures Jev against the showcase project's labelled examples.
///
/// `check` reads the examples and checks them against the showcase
/// project, offline.
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
        /// The folder of labelled examples [default: showcase/labels]
        #[arg(long)]
        labels: Option<PathBuf>,
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
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
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

/// Reads every `.toml` file in `dir`, in file-name order.
pub fn load(dir: &Path) -> Result<Labels, ToolError> {
    let read = |path: &Path, source| ToolError::Read {
        path: path.to_owned(),
        source,
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|source| read(dir, source))? {
        let path = entry.map_err(|source| read(dir, source))?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = fs::read_to_string(&path).map_err(|source| read(&path, source))?;
            files.push((name, text));
        }
    }
    if files.is_empty() {
        return Err(ToolError::NoFiles {
            dir: dir.to_owned(),
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
    let by_agent = items
        .iter()
        .filter(|item| item.label().by() == &Labeller::Agent)
        .count();
    let mut summary = format!(
        "{} examples, {} labels, {by_agent} of them by an agent, as seeds still to review.\n",
        prepared.calls().len(),
        items.len(),
    );
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
        Job::QuestionCheck => "Question check (checks 1 to 5)",
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
    fn a_folder_without_examples_says_so() {
        let error = load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))).unwrap_err();

        assert!(matches!(error, ToolError::NoFiles { .. }), "{error}");
    }
}
