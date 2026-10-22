//! Measuring the base prompts live: Generate questions on one showcase
//! change and one area through the real Jev and generator, at the app seam,
//! under the default cut-offs (its config sets no `[cut_offs]`), then print
//! one row of what became of the candidates. The rows of several pairs make the before and after
//! tables of a change to the base prompts.
//!
//! A row is read from the app's own log lines and the screen, so it shows
//! only names, counts, bands and costs, never a question's text:
//!
//! - whether the relevance gate passed the area; when it didn't, the row is
//!   measured after Ask anyway;
//! - the candidates written, and how many the plain checks dropped;
//! - for each check, how many candidates it withheld, by band;
//! - the questions shown, and how many criteria the show rule dropped from
//!   the questions that passed, as asking for more than they need;
//! - the generator's tokens, how many calls Jev answered, and what the run
//!   cost.
//!
//! Each pair calls `OpenRouter` with a real key, about 0.12 to 0.15 USD a
//! run with the base prompts of #119, so every test that does is ignored and
//! runs only when asked:
//!
//! ```sh
//! OPENROUTER_API_KEY=… cargo nextest run -p whetstone --locked --no-capture --run-ignored only -E 'test(/measure_questions::/)'
//! ```
//!
//! None is named `live_…`, so the keyed end-to-end job never runs them.
//! Per-PR tests never call `OpenRouter` (rule 48): the one test here that
//! isn't ignored reads a row from what the app logs while a scripted AI
//! answers, so a change to those log lines shows.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};

use whetstone_ai::ScriptedAi;
use whetstone_core::CandidateCheck;

use crate::live_practice::live_key;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, generate, generate_in, stems,
};
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
    let change_at = showcase
        .changes()
        .iter()
        .position(|change| change.summary() == subject)
        .expect("the showcase has a change with this subject");
    let area_at = showcase
        .areas()
        .iter()
        .position(|offered| offered.id().as_str() == area)
        .expect("the showcase offers this area");
    (change_at, area_at)
}

/// Runs `run` with this thread's log lines captured, and returns what it
/// returned with the lines. Every job runs on the test's thread, so the
/// app and the AI port alike write to this thread's subscriber.
fn capture<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let captured = Captured::default();
    let writer = captured.clone();
    let returned = tracing::subscriber::with_default(
        tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish(),
        run,
    );
    (returned, captured.lines())
}

/// Generates questions about `area` on the showcase change `subject`, with
/// the fewest questions, so that one generator call writes five candidates.
/// Returns whether the relevance gate passed the area, and the log lines.
fn generate_live(subject: &str, area: &str) -> (&'static str, Vec<String>) {
    let key = live_key();
    let (change, place) = places(subject, area);
    capture(|| {
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
        if session
            .text()
            .contains("Nothing to practice in this change")
        {
            session.press("j j <Enter>");
            session.settle();
            "dropped (Ask anyway)"
        } else {
            "passed"
        }
    })
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

/// The number in the field `name` of `line`, if it has one. A field that
/// is `None` isn't logged at all.
fn number<T: std::str::FromStr>(line: &str, name: &str) -> Option<T> {
    field(line, name).and_then(|value| value.parse().ok())
}

/// The count in the field `name` of `line`.
fn count(line: &str, name: &str) -> usize {
    number(line, name).expect("the field is a count")
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

/// Measures `area` on the showcase change `subject`, as one [`row`].
fn measure(subject: &str, area: &str) -> String {
    let (gate, lines) = generate_live(subject, area);
    row(subject, area, gate, &lines)
}

/// One Markdown row from the log `lines` of Generate questions about `area`
/// on the change `subject`: the change, the area, the `gate`, the
/// candidates written, the plain-check drops, the candidates withheld, each
/// check's withheld count by band, the questions shown, the criteria dropped
/// from the questions that passed, the generator's input and output tokens,
/// the calls Jev answered, and the cost of every request in USD.
fn row(subject: &str, area: &str, gate: &str, lines: &[String]) -> String {
    let lines_with = |message: &str| -> Vec<&String> {
        lines.iter().filter(|line| line.contains(message)).collect()
    };
    let (written, dropped) = lines_with("the generator wrote candidates").iter().fold(
        (0, 0),
        |(written, dropped), line| {
            (
                written + count(line, "written"),
                dropped + count(line, "dropped"),
            )
        },
    );
    let withheld = lines_with("withheld a candidate");
    let mut checks = Vec::new();
    for (check, bands) in withheld_by_check(&withheld) {
        let total: usize = bands.values().sum();
        let bands: Vec<String> = bands
            .iter()
            .map(|(band, count)| format!("{band} {count}"))
            .collect();
        checks.push(format!("{check} {total} ({})", bands.join(", ")));
    }
    let picked = lines_with("picked the questions");
    let picked = picked.last().expect("the app picked questions, or none");
    let (shown, criteria_dropped) = (count(picked, "picked"), count(picked, "criteria_dropped"));
    let replies = lines_with("OpenRouter replied");
    let generator = replies
        .iter()
        .filter(|line| field(line, "role") == Some("Generator"));
    let tokens = |name: &str| -> usize {
        generator
            .clone()
            .filter_map(|line| number::<usize>(line, name))
            .sum()
    };
    let jev_calls = replies
        .iter()
        .filter(|line| field(line, "role") == Some("Jev"))
        .count();
    // Summing no costs gives -0.0, so the total starts at 0.0.
    let cost = replies
        .iter()
        .filter_map(|line| number::<f64>(line, "cost_usd"))
        .fold(0.0, |total, cost| total + cost);
    format!(
        "| {subject} | {area} | {gate} | {written} | {dropped} | {} | {} | {shown} | {criteria_dropped} | {} / {} | {jev_calls} | {cost:.4} |",
        withheld.len(),
        checks.join("; "),
        tokens("input_tokens"),
        tokens("output_tokens"),
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

#[test]
fn a_row_reads_what_the_app_logs_about_each_candidate() {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 3);
    let mut written: Vec<_> = stems
        .iter()
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written.push(candidate("Malformed?", "D", PAYMENT_LIFECYCLE));
    // The first passes, but its first criterion asks for more than it
    // needs, so the question keeps its second.
    let criterion = written[0].rubric[0].clone();
    written[0].rubric.push(criterion);
    ai.on_write(&area_id(DOMAIN), Ok(written));
    ai.on_check_passing_except(
        &stems[0],
        [answer(CandidateCheck::CriterionMinimum(0), 0.1)],
    );
    ai.on_check_passing_except(
        &stems[1],
        [
            answer(CandidateCheck::NoGiveaway, 0.5),
            answer(CandidateCheck::NeedsChange, 0.1),
        ],
    );
    ai.on_check_passing_except(&stems[2], [answer(CandidateCheck::NoGiveaway, 0.4)]);

    let ((), lines) = capture(|| {
        generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32).settle();
    });

    // A scripted AI logs none of its replies: no tokens, Jev calls or cost.
    assert_eq!(
        row("Model the payment lifecycle", "domain", "passed", &lines),
        "| Model the payment lifecycle | domain | passed | 4 | 1 | 2 | NeedsChange 1 (ClearNo 1); \
         NoGiveaway 2 (Unclear 2) | 1 | 1 | 0 / 0 | 0 | 0.0000 |"
    );
}
