//! Measuring the base prompts live: Generate questions on one showcase
//! change and one area through the real Jev and generator, at the app seam,
//! under the assessment policy in use, then print one row of what became of
//! the candidates. The rows of several pairs make the before and after
//! tables of a change to the base prompts.
//!
//! A row is read from the app's own log lines and the screen, so it shows
//! only names, counts, bands and costs, never a question's text:
//!
//! - whether the relevance gate passed the area; when it didn't, the row is
//!   measured after Ask anyway;
//! - the candidates written, and how many the plain checks dropped;
//! - for each check, how many candidates it withheld, by band;
//! - the questions shown, and what the run cost.
//!
//! Each pair calls `OpenRouter` with a real key, about 0.08 USD a run, so
//! every test here is ignored and runs only when asked:
//!
//! ```sh
//! OPENROUTER_API_KEY=… cargo nextest run -p whetstone --locked --run-ignored only -E 'test(/measure_questions::/)'
//! ```
//!
//! None is named `live_…`, so the keyed end-to-end job never runs them.
//! Per-PR tests never call `OpenRouter` (rule 48).

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};

use crate::live_practice::live_key;
use crate::preparing_questions::generate_in;
use crate::session::Setup;

/// The log lines the app writes while a test runs, as text.
#[derive(Debug, Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("no test thread panics while writing the log")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(
            &self
                .0
                .lock()
                .expect("no test thread panics while writing the log"),
        )
        .lines()
        .map(str::to_owned)
        .collect()
    }
}

/// The place of the showcase change whose subject is `subject`, and of the
/// area whose ID is `area`, as the picker lists them.
fn places(subject: &str, area: &str) -> (usize, usize) {
    let showcase = whetstone::embedded_showcase();
    let change = showcase
        .changes()
        .iter()
        .position(|change| change.summary() == subject)
        .expect("the showcase has a change with this subject");
    let area = showcase
        .areas()
        .iter()
        .position(|offered| offered.id().as_str() == area)
        .expect("the showcase offers this area");
    (change, area)
}

/// Generates questions about `area` on the showcase change `subject`, with
/// the fewest questions, so that one generator call writes five candidates.
/// Returns whether the relevance gate passed the area, and the log lines.
fn generate_live(subject: &str, area: &str) -> (&'static str, Vec<String>) {
    let key = live_key();
    let captured = Captured::default();
    let writer = captured.clone();
    // Every job runs on the test's thread, so this thread's subscriber hears
    // the app and the AI port alike.
    let _log = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish(),
    );
    let (change, place) = places(subject, area);
    let mut session = generate_in(
        Setup {
            config: Some("questions = 3\n"),
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", &key)],
            live: true,
            ..Setup::default()
        },
        &[place],
        change,
        120,
        32,
    );
    session.settle();
    let gate = if session
        .text()
        .contains("Nothing to practice in this change")
    {
        session.press("j j <Enter>");
        session.settle();
        "dropped (Ask anyway)"
    } else {
        "passed"
    };
    (gate, captured.lines())
}

/// The value of the field `name` in a log line: up to the next space, or
/// to the end of the list it starts.
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (_, rest) = line.split_once(&format!(" {name}="))?;
    if rest.starts_with('[') {
        rest.split_inclusive(']').next()
    } else {
        rest.split(' ').next()
    }
}

/// The count in the field `name` of `line`.
fn count(line: &str, name: &str) -> usize {
    field(line, name)
        .and_then(|value| value.parse().ok())
        .expect("the field is a count")
}

/// For each check that withheld a candidate, how many it withheld in each
/// band, from the lines "withheld a candidate", whose `failed` field lists
/// `(check, band)` pairs.
fn withheld_by_check(withheld: &[&String]) -> BTreeMap<String, BTreeMap<String, usize>> {
    let mut by_check: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for line in withheld {
        let failed = field(line, "failed").expect("a withheld line names its failed checks");
        for check in failed
            .trim_matches(|bracket| bracket == '[' || bracket == ']')
            .split("), (")
        {
            let (name, band) = check
                .trim_matches(|paren| paren == '(' || paren == ')')
                .rsplit_once(", ")
                .expect("each failed check is a (check, band) pair");
            *by_check
                .entry(name.to_owned())
                .or_default()
                .entry(band.replace("Some(", "").replace(')', ""))
                .or_default() += 1;
        }
    }
    by_check
}

/// Measures `area` on the showcase change `subject`, as one Markdown row:
/// the change, the area, the gate, the candidates written, the plain-check
/// drops, the candidates withheld, each check's withheld count by band, the
/// questions shown and the cost in USD.
fn measure(subject: &str, area: &str) -> String {
    let (gate, lines) = generate_live(subject, area);
    let with = |message: &str| -> Vec<&String> {
        lines.iter().filter(|line| line.contains(message)).collect()
    };
    let (written, dropped) =
        with("the generator wrote candidates")
            .iter()
            .fold((0, 0), |(written, dropped), line| {
                (
                    written + count(line, "written"),
                    dropped + count(line, "dropped"),
                )
            });
    let withheld = with("withheld a candidate");
    let mut checks = Vec::new();
    for (check, bands) in withheld_by_check(&withheld) {
        let total: usize = bands.values().sum();
        let bands: Vec<String> = bands
            .iter()
            .map(|(band, count)| format!("{band} {count}"))
            .collect();
        checks.push(format!("{check} {total} ({})", bands.join(", ")));
    }
    let shown = with("picked the questions")
        .last()
        .map(|line| count(line, "picked"))
        .expect("the app picked questions, or none");
    let cost: f64 = with("OpenRouter replied")
        .iter()
        .filter_map(|line| field(line, "cost_usd"))
        .filter_map(|value| {
            value
                .trim_start_matches("Some(")
                .trim_end_matches(')')
                .parse::<f64>()
                .ok()
        })
        .sum();
    format!(
        "| {subject} | {area} | {gate} | {written} | {dropped} | {} | {} | {shown} | {cost:.4} |",
        withheld.len(),
        checks.join("; "),
    )
}

#[test]
#[ignore = "calls OpenRouter with a real key"]
fn measure_payment_lifecycle_in_domain() {
    println!("{}", measure("Model the payment lifecycle", "domain"));
}

#[test]
#[ignore = "calls OpenRouter with a real key"]
fn measure_payment_lifecycle_in_system_design() {
    println!(
        "{}",
        measure("Model the payment lifecycle", "system-design")
    );
}

#[test]
#[ignore = "calls OpenRouter with a real key"]
fn measure_serve_over_http_in_domain() {
    println!("{}", measure("Serve payments over HTTP", "domain"));
}

#[test]
#[ignore = "calls OpenRouter with a real key"]
fn measure_serve_over_http_in_infrastructure() {
    println!("{}", measure("Serve payments over HTTP", "infrastructure"));
}

#[test]
#[ignore = "calls OpenRouter with a real key"]
fn measure_cache_per_tenant_in_scalability() {
    println!("{}", measure("Cache payments per tenant", "scalability"));
}

#[test]
#[ignore = "calls OpenRouter with a real key"]
fn measure_cache_per_tenant_in_infrastructure() {
    println!("{}", measure("Cache payments per tenant", "infrastructure"));
}
