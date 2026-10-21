//! The summary, once the learner finishes a Practice: what happened to its
//! questions, each question's outcomes, and what the session cost.
//!
//! It counts the questions answered with reasoning and those answered with
//! a choice only, separately, the questions skipped and that can't be
//! answered from this context, and those whose reasoning awaits evaluation,
//! that were assisted attempts and that were reported. A question still
//! unattempted when the learner finished counts as skipped, whether Skip
//! moved past it or Finish came first; one skipped and answered later
//! counts as answered. Each question is listed with its choice outcome and
//! each criterion's outcome, separately: there is no combined grade. A
//! question answered with a choice only has no criterion outcomes: its
//! reasoning wasn't written, so it wasn't assessed.
//!
//! The cost, under the counts, is what each reply reported, added up for
//! Jev, for the generator and for both. A request whose reply reported no
//! cost, or that failed or was cancelled, is counted instead, as "not
//! reported for *n* requests", beside its role and the total: the cost is
//! never estimated, so a role whose requests reported none never looks
//! free.
//!
//! Finishing sends no new request. One still running when the learner
//! finished runs on: the summary says how many still run, and once one
//! replies, it shows its outcome and its cost. Home leads back to home,
//! which stops any still running.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{List, Paragraph};
use whetstone_core::{ChoiceOutcome, Cost, CriterionOutcome, Question, SessionCost};

use crate::columns;
use crate::feedback::{self, AttemptKind, Evaluation, Feedback};
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap};
use crate::questions::Questions;
use crate::sent_requests::Spending;
use crate::theme::{ColorSupport, ControlState, Theme};
use crate::view::Route;

/// The summary's keys.
pub(crate) const KEYMAP: Keymap = &[
    Binding {
        does: "Scroll the summary down or up",
        ..keymap::MOVE
    },
    Binding {
        does: "Scroll the summary down or up half a page",
        hint: Some("Ctrl+d/u scroll"),
        ..keymap::HALF_PAGE
    },
    Binding {
        does: "Scroll to the top or bottom of the summary",
        ..keymap::ENDS
    },
    Binding {
        does: "Go home",
        hint: Some("Enter home"),
        ..keymap::ACTIVATE
    },
    Binding {
        does: "Go back to the areas and changes",
        ..keymap::BACK
    },
];

/// The one control, which leads back to home.
const HOME: &str = "Home";

/// A question as the summary lists it.
#[derive(Debug)]
pub(crate) struct Listed<'a> {
    pub(crate) question: &'a Question,
    /// Its area's name.
    pub(crate) area_name: &'a str,
    /// Whether the attempt was the learner's own, or assisted.
    pub(crate) kind: AttemptKind,
    /// How many times the learner reported it.
    pub(crate) reports: usize,
    pub(crate) attempt: Ended<'a>,
}

/// Where a question's attempt stood as the learner finished.
#[derive(Debug)]
pub(crate) enum Ended<'a> {
    /// Still unattempted: it counts as skipped.
    Skipped,
    /// The learner said it can't be answered from this context.
    CannotAnswer,
    /// A response was submitted, with this feedback on it.
    Answered(&'a Feedback),
}

/// The summary of a finished Practice.
#[derive(Debug)]
pub(crate) struct Summary {
    /// The questions, whose outcomes still come in from the requests
    /// running when the learner finished.
    questions: Questions,
    /// The change and the chosen areas, as Practice names them.
    subject: String,
    /// The row at the top of the summary.
    top: Position,
}

impl Summary {
    /// The summary of `questions`, about `subject`, the change and the
    /// chosen areas, whose requests cost what `spending` says so far.
    pub(crate) fn new(questions: Questions, subject: String, spending: Spending) -> Self {
        let summary = Self {
            questions,
            subject,
            top: Position::default(),
        };
        let counts = summary.counts();
        tracing::info!(
            questions = counts.questions,
            answered_with_reasoning = counts.answered_with_reasoning,
            choice_only = counts.choice_only,
            skipped = counts.skipped,
            cannot_answer = counts.cannot_answer,
            awaiting_evaluation = counts.awaiting_evaluation,
            assisted = counts.assisted,
            reported = counts.reported,
            jev_usd = spending.total.jev().usd(),
            generator_usd = spending.total.generator().usd(),
            not_reported = spending.total.not_reported(),
            running = spending.running,
            "finished the Practice"
        );
        summary
    }

    /// The questions, for the results still to come.
    pub(crate) fn questions_mut(&mut self) -> &mut Questions {
        &mut self.questions
    }

    /// How many questions ended each way.
    fn counts(&self) -> Counts {
        let listed = self.questions.listed();
        let count = |counted: fn(&Listed<'_>) -> bool| listed.iter().filter(|q| counted(q)).count();
        let answered = count(|listed| matches!(listed.attempt, Ended::Answered(_)));
        let choice_only = count(|listed| {
            matches!(
                listed.attempt,
                Ended::Answered(feedback) if matches!(feedback.evaluation(), Evaluation::NotWritten)
            )
        });
        Counts {
            questions: listed.len(),
            answered_with_reasoning: answered - choice_only,
            choice_only,
            skipped: count(|listed| matches!(listed.attempt, Ended::Skipped)),
            cannot_answer: count(|listed| matches!(listed.attempt, Ended::CannotAnswer)),
            awaiting_evaluation: count(|listed| {
                matches!(
                    listed.attempt,
                    Ended::Answered(feedback)
                        if matches!(feedback.evaluation(), Evaluation::Unsent | Evaluation::Awaiting)
                )
            }),
            assisted: count(|listed| listed.kind == AttemptKind::Assisted),
            reported: count(|listed| listed.reports > 0),
        }
    }

    /// Applies `command` to the summary drawn in `area`, with what the
    /// session's requests cost so far, `spending`, and returns what it asks
    /// of the app, if anything: Home goes home.
    pub(crate) fn apply(
        &mut self,
        command: Command,
        area: Rect,
        spending: Spending,
    ) -> Option<Command> {
        match command {
            Command::Activate => return Some(Command::Go(Route::Home)),
            Command::Down
            | Command::Up
            | Command::HalfPageDown
            | Command::HalfPageUp
            | Command::Top
            | Command::Bottom => {
                // Styles do not change how the text wraps.
                let lines = self.lines(&Theme::new(ColorSupport::NoColor), area.width, spending);
                let [body, ..] = layout(lines.len(), area);
                let last = lines.len().saturating_sub(usize::from(body.height));
                self.top = self.top.scrolled(command, last, usize::from(body.height));
            }
            _ => {}
        }
        None
    }

    /// The summary's text, drawn with `theme`, `width` cells wide, with what
    /// the session's requests cost so far, `spending`: the counts, the cost,
    /// then each question.
    fn lines(&self, theme: &Theme, width: u16, spending: Spending) -> Vec<Line<'static>> {
        let width = usize::from(width);
        let mut lines = vec![Line::styled("Summary", theme.title)];
        lines.extend(
            columns::wrap(&self.subject, width)
                .into_iter()
                .map(|line| Line::styled(line, theme.muted)),
        );
        lines.push(Line::default());
        let counts = self.counts();
        let rows = [
            ("Answered with reasoning", counts.answered_with_reasoning),
            ("Choice only", counts.choice_only),
            ("Skipped", counts.skipped),
            ("Cannot answer from this context", counts.cannot_answer),
            ("Awaiting evaluation", counts.awaiting_evaluation),
            ("Assisted", counts.assisted),
            ("Reported", counts.reported),
        ];
        lines.extend(in_columns(
            rows.map(|(label, count)| (label, count.to_string())),
        ));
        lines.push(Line::default());
        lines.extend(cost_lines(spending, theme, width));
        for (place, listed) in self.questions.listed().iter().enumerate() {
            lines.push(Line::default());
            lines.extend(question_lines(place, listed, theme, width));
        }
        lines
    }

    /// Draws the summary in `area` with `theme`, with what the session's
    /// requests cost so far, `spending`.
    pub(crate) fn render(
        &self,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
        spending: Spending,
    ) {
        let lines = self.lines(theme, area.width, spending);
        let [body, _, buttons] = layout(lines.len(), area);
        let last = lines.len().saturating_sub(usize::from(body.height));
        let top = self.top.index().min(last);
        frame.render_widget(
            Paragraph::new(lines).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
            body,
        );
        frame.render_widget(
            List::new([theme.control(
                HOME,
                ControlState {
                    focused: true,
                    ..ControlState::default()
                },
            )]),
            buttons,
        );
    }
}

/// The lines listing the question at `place`, from 0, as `listed` says it
/// ended, drawn with `theme`, `width` cells wide: its place and area, then,
/// indented, its stem and what became of it. An answered question has its
/// choice outcome, then each criterion's outcome, or where its evaluation
/// stands, each on a line of its own, with no combined grade.
fn question_lines(
    place: usize,
    listed: &Listed<'_>,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let assisted = match listed.kind {
        AttemptKind::Own => "",
        AttemptKind::Assisted => feedback::ASSISTED,
    };
    let mut heading = format!("Question {} · {}{assisted}", place + 1, listed.area_name);
    if listed.reports > 0 {
        heading.push_str(" · Reported");
    }
    let mut lines = vec![Line::styled(heading, theme.title)];
    let indented = |lines: &mut Vec<Line<'static>>, text: &str| {
        lines.extend(
            columns::wrap(text, width.saturating_sub(INDENT))
                .into_iter()
                .map(|line| Line::raw(format!("{}{line}", " ".repeat(INDENT)))),
        );
    };
    indented(&mut lines, listed.question.stem());
    match listed.attempt {
        Ended::Skipped => indented(&mut lines, "Skipped"),
        Ended::CannotAnswer => indented(&mut lines, "Cannot answer from this context"),
        Ended::Answered(feedback) => {
            let choice = match feedback.outcome().outcome() {
                ChoiceOutcome::Supported => "Supported",
                ChoiceOutcome::NotSupported => "Not supported",
            };
            indented(&mut lines, &format!("Choice · {choice}{assisted}"));
            match feedback.evaluation() {
                Evaluation::NotWritten => {
                    indented(&mut lines, "Reasoning not written: not assessed");
                }
                Evaluation::Unsent | Evaluation::Awaiting => {
                    indented(&mut lines, "Reasoning · Awaiting evaluation");
                }
                Evaluation::Unavailable(_) => {
                    indented(&mut lines, "Reasoning · Evaluation unavailable");
                }
                Evaluation::Assessed(outcomes) => {
                    for (criterion, assessed) in listed.question.rubric().iter().zip(outcomes) {
                        let outcome = match assessed.outcome() {
                            CriterionOutcome::Met => "Met",
                            CriterionOutcome::NotMet => "Not met",
                            CriterionOutcome::NeedsReview => "Needs review",
                        };
                        indented(
                            &mut lines,
                            &format!("{outcome}{assisted} · {}", criterion.statement()),
                        );
                    }
                }
            }
        }
    }
    lines
}

/// The lines giving what the session's requests cost so far, `spending`,
/// drawn with `theme`, `width` cells wide: what Jev's replies and the
/// generator's reported, and both together, each with how many of its
/// requests reported none, then how many requests still run.
fn cost_lines(spending: Spending, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let row = |cost: Cost, of: SessionCost| match of.not_reported() {
        0 => dollars(cost),
        1 => format!("{} · not reported for 1 request", dollars(cost)),
        requests => format!("{} · not reported for {requests} requests", dollars(cost)),
    };
    let mut lines = vec![Line::styled("Cost, as each reply reported it", theme.title)];
    lines.extend(in_columns([
        ("Jev", row(spending.jev.jev(), spending.jev)),
        (
            "Generator",
            row(spending.generator.generator(), spending.generator),
        ),
        ("Total", row(spending.total.total(), spending.total)),
    ]));
    let running = match spending.running {
        0 => None,
        1 => Some("1 request is still running: its cost is added when it replies.".to_owned()),
        requests => Some(format!(
            "{requests} requests are still running: their cost is added when they reply."
        )),
    };
    if let Some(running) = running {
        lines.extend(
            columns::wrap(&running, width)
                .into_iter()
                .map(|line| Line::styled(line, theme.muted)),
        );
    }
    lines
}

/// Each of `rows`, a label and what it labels, in two columns: what each
/// label labels starts in one column, past the widest label.
fn in_columns<const N: usize>(rows: [(&str, String); N]) -> Vec<Line<'static>> {
    let column = columns::next_column(rows.iter().map(|(label, _)| *label));
    rows.into_iter()
        .map(|(label, text)| {
            let mut line = Line::raw(label.to_owned());
            columns::pad_to(&mut line, column);
            line.push_span(text);
            line
        })
        .collect()
}

/// `cost` in dollars, to the millionth of a dollar, without the zeros that
/// end it past the cents: `$0.25`, `$0.077414`. A cost too small to show
/// that way shows as less than a millionth, never as nothing.
fn dollars(cost: Cost) -> String {
    let usd = cost.usd();
    let shown = format!("{usd:.6}");
    if usd > 0.0 && shown == "0.000000" {
        return "< $0.000001".to_owned();
    }
    // Every number formatted with six decimals has a point.
    let (whole, fraction) = shown.split_once('.').unwrap_or((&shown, ""));
    let fraction = fraction.trim_end_matches('0');
    format!("${whole}.{fraction:0<2}")
}

/// How many cells the lines under each question's heading are indented.
const INDENT: usize = 2;

/// How many questions ended each way, and how many there are.
#[derive(Debug, Clone, Copy)]
struct Counts {
    questions: usize,
    /// Answered with reasoning written.
    answered_with_reasoning: usize,
    /// Answered with a choice alone, so its reasoning isn't assessed.
    choice_only: usize,
    skipped: usize,
    cannot_answer: usize,
    awaiting_evaluation: usize,
    assisted: usize,
    reported: usize,
}

/// Where the summary's text of `lines` rows, a blank row and its control go
/// in `area`. The control keeps its row on a short screen, and the text
/// scrolls above it.
fn layout(lines: usize, area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Max(u16::try_from(lines).unwrap_or(u16::MAX)),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area)
}
