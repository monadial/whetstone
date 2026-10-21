//! Review withheld questions: the candidates the question check withheld,
//! one at a time and read-only, under "Withheld: not shown in Practice". Each
//! shows its area, its stem and options, and each check that withheld it,
//! in plain words, with Jev's probability and band, under the label Inspect
//! Jev's answers uses. Then its answer key, the rationales, "why it
//! matters", the rubric, the hint and what it cites. Inspect source reads
//! the evidence it rests on, exactly as it was sent.
//!
//! For each check that withheld the candidate, the learner votes Rightly
//! withheld or Should have passed, with an optional note, or skips it (see
//! [`crate::vote`]). Each vote is saved as a labelled example in the
//! learner's labels file (see [`crate::labels_file`]) by a background job.
//! A vote saved is final for the run. Voting never changes an outcome or
//! brings a question into Practice: a withheld question is never answered or
//! assessed.
//!
//! It opens from No questions passed checks, and from the palette on the
//! questions while any candidate was withheld.

use std::collections::BTreeSet;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph};
use whetstone_core::{CandidateCheck, Extent, OptionLetter, Withheld};

use crate::columns;
use crate::focus::Position;
use crate::jev_answers;
use crate::job::{JobOutput, Jobs};
use crate::keymap::{self, Binding, Command, Input, Keymap};
use crate::labels_file::{Cast, LabelsFile, Vote, VoteState, VotedChange};
use crate::palette::Action;
use crate::reader::Reader;
use crate::theme::{ColorSupport, ControlState, Theme};
use crate::view::{Context, Reading, Route, View};
use crate::vote::{self, VoteForm, Voting};

/// What heads every withheld candidate.
const WITHHELD: &str = "Withheld: not shown in Practice";

/// The review's own keys.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    keymap::ACTIVATE,
    Binding {
        does: "Scroll the withheld question down or up half a page",
        hint: Some("Ctrl+d/u scroll"),
        ..keymap::HALF_PAGE
    },
    Binding {
        does: "Scroll to the top or bottom of the withheld question",
        ..keymap::ENDS
    },
    Binding {
        does: "Go back to Practice",
        ..keymap::BACK
    },
];

/// The review's own actions in the palette.
const ACTIONS: &[Action] = &[
    Action {
        label: "Inspect source",
        does: "Read the evidence the withheld questions rest on",
        command: Command::Go(Route::Read(Reading::Evidence)),
    },
    Action {
        label: "Back",
        does: "Go back to Practice",
        command: Command::Back,
    },
];

/// The review's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// Votes on the check at this place among those that withheld the
    /// candidate.
    Vote(usize),
    InspectSource,
    /// The next withheld candidate, coming round after the last.
    NextWithheld,
    Back,
}

impl Control {
    fn label(self) -> String {
        match self {
            Self::Vote(place) => format!("Vote on check {}", place + 1),
            Self::InspectSource => "Inspect source".to_owned(),
            Self::NextWithheld => "Next withheld question".to_owned(),
            Self::Back => "Back".to_owned(),
        }
    }
}

/// One withheld candidate, with its area's name.
#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) withheld: Withheld,
    pub(crate) area_name: String,
}

/// The review of the candidates the question check withheld.
#[derive(Debug)]
pub(crate) struct ReviewWithheld {
    /// Each withheld candidate, in the order written: never none.
    candidates: Vec<Entry>,
    /// The place of the candidate that shows.
    at: usize,
    focus: Position,
    /// The row at the top of the text.
    top: Position,
    /// The checks the learner skipped, each by its candidate's place.
    skipped: BTreeSet<(usize, CandidateCheck)>,
    /// The vote being cast, if the box is open, on the check at this place.
    voting: Option<(usize, Box<VoteForm>)>,
    /// The votes cast, waiting for a job to save each.
    to_save: Vec<Cast>,
    /// The change the candidates were written about.
    change: VotedChange,
    /// The reader of the evidence the candidates rest on.
    evidence: Reader,
    labels: Arc<LabelsFile>,
}

impl ReviewWithheld {
    /// The review of `candidates`, written about `change` from the evidence
    /// `evidence` reads, saving votes in `labels`, or none when none was
    /// withheld.
    pub(crate) fn new(
        candidates: Vec<Entry>,
        change: VotedChange,
        evidence: Reader,
        labels: &Arc<LabelsFile>,
    ) -> Option<Self> {
        if candidates.is_empty() {
            return None;
        }
        tracing::info!(
            withheld = candidates.len(),
            "opened the review of withheld questions"
        );
        Some(Self {
            candidates,
            at: 0,
            focus: Position::default(),
            top: Position::default(),
            skipped: BTreeSet::new(),
            voting: None,
            to_save: Vec::new(),
            change,
            evidence,
            labels: Arc::clone(labels),
        })
    }

    fn current(&self) -> &Entry {
        &self.candidates[self.at]
    }

    /// The controls for the candidate that shows, in focus order.
    fn controls(&self) -> Vec<Control> {
        (0..self.current().withheld.failed().len())
            .map(Control::Vote)
            .chain([Control::InspectSource, Control::NextWithheld, Control::Back])
            .collect()
    }

    fn focused(&self) -> Control {
        let controls = self.controls();
        controls
            .get(self.focus.index())
            .or(controls.last())
            .copied()
            .unwrap_or(Control::Back)
    }

    /// Why `control` is disabled, if it is.
    fn disabled(&self, control: Control) -> Option<&'static str> {
        match control {
            Control::Vote(place) => {
                if self.labels.shown().is_none() {
                    return Some("there's no labels file to save it in");
                }
                let check = self.current().withheld.failed()[place];
                match self.vote(check) {
                    Some(VoteState::Saving(_)) => Some("saving"),
                    Some(VoteState::Saved(_)) => Some("voted"),
                    Some(VoteState::Unsaved(..)) | None => None,
                }
            }
            Control::NextWithheld => (self.candidates.len() == 1).then_some("it's the only one"),
            Control::InspectSource | Control::Back => None,
        }
    }

    /// Where the vote on `check` of the candidate that shows stands.
    fn vote(&self, check: CandidateCheck) -> Option<VoteState> {
        self.labels.vote(self.current().withheld.candidate(), check)
    }

    /// `check` of the candidate that shows, in plain words, and Jev's
    /// answer to it against what the show rule needed.
    fn banded(&self, check: CandidateCheck) -> (String, String) {
        let withheld = &self.current().withheld;
        let needed = withheld
            .passing(check)
            .iter()
            .map(|&band| jev_answers::band_name(band))
            .collect::<Vec<_>>()
            .join(" or ");
        let banded = match (withheld.answers().get(check), withheld.band(check)) {
            (Some(answer), Some(band)) => {
                format!(
                    "{} · {} · it needed {needed}",
                    answer.get(),
                    jev_answers::band_name(band)
                )
            }
            _ => format!("Jev didn't answer it · it needed {needed}"),
        };
        (jev_answers::statement(check), banded)
    }

    /// Applies what the learner did in the box on the check at `place`.
    fn voted(&mut self, place: usize, voting: Voting) {
        let check = self.current().withheld.failed()[place];
        match voting {
            Voting::Open => return,
            Voting::Cancelled => {}
            Voting::Skipped => {
                self.skipped.insert((self.at, check));
            }
            Voting::Voted(verdict, note) => {
                self.skipped.remove(&(self.at, check));
                let withheld = &self.current().withheld;
                tracing::info!(noted = note.is_some(), "cast a vote on a withheld question");
                self.to_save.push(self.labels.cast(Vote {
                    change: self.change.clone(),
                    candidate: withheld.candidate().clone(),
                    check,
                    needed: withheld.needed(check),
                    verdict,
                    note,
                }));
            }
        }
        self.voting = None;
    }

    /// Opens the box to vote on the check at `place`.
    fn open_vote(&mut self, place: usize) {
        let check = self.current().withheld.failed()[place];
        let (statement, banded) = self.banded(check);
        self.voting = Some((
            place,
            Box::new(VoteForm::new(
                place + 1,
                statement,
                format!("Jev's band: {banded}"),
                self.labels.shown().unwrap_or_default().to_owned(),
            )),
        ));
    }

    /// The text of the candidate that shows, drawn with `theme`, `width`
    /// cells wide.
    fn lines(&self, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        let width = usize::from(width);
        let entry = self.current();
        let candidate = entry.withheld.candidate();
        let mut lines = Vec::new();
        let wrapped = |lines: &mut Vec<Line<'static>>, text: &str| {
            lines.extend(columns::wrap(text, width).into_iter().map(Line::raw));
        };
        let heading = |lines: &mut Vec<Line<'static>>, text: &str| {
            lines.push(Line::default());
            lines.push(Line::styled(text.to_owned(), theme.title));
        };
        wrapped(&mut lines, candidate.stem());
        lines.push(Line::default());
        for letter in OptionLetter::ALL {
            let lead = format!("{letter}  ");
            for (index, text) in columns::wrap(candidate.option(letter), width.saturating_sub(3))
                .into_iter()
                .enumerate()
            {
                lines.push(if index == 0 {
                    Line::raw(format!("{lead}{text}"))
                } else {
                    Line::raw(format!("   {text}"))
                });
            }
        }
        heading(&mut lines, "Checks that withheld it");
        lines.extend(self.check_lines(theme, width));
        let key = candidate.answer_key();
        heading(&mut lines, &format!("Answer key: {key}"));
        for letter in OptionLetter::ALL {
            heading(&mut lines, &format!("Rationale for {letter}"));
            wrapped(&mut lines, candidate.rationale(letter));
        }
        heading(&mut lines, "Why it matters");
        wrapped(&mut lines, candidate.why_it_matters());
        let count = candidate.rubric().len();
        for (index, criterion) in candidate.rubric().iter().enumerate() {
            heading(
                &mut lines,
                &format!("Rubric, criterion {} of {count}", index + 1),
            );
            wrapped(&mut lines, criterion.statement());
            wrapped(&mut lines, &format!("Met: {}", criterion.met()));
            wrapped(&mut lines, &format!("Not met: {}", criterion.not_met()));
        }
        heading(&mut lines, "Hint");
        wrapped(&mut lines, candidate.hint().text());
        heading(&mut lines, "Cited");
        for citation in candidate.citations() {
            wrapped(
                &mut lines,
                &format!("{}, {}", citation.path(), Extent::Lines(citation.lines())),
            );
        }
        lines.push(Line::default());
        let kept = match self.labels.shown() {
            Some(file) => format!(
                "Your votes are kept in {file}, on this machine only, and are never sent \
                 anywhere. On your own repository, they hold private code from it."
            ),
            None => {
                "Votes can't be saved: Whetstone has no state folder to keep them in.".to_owned()
            }
        };
        lines.extend(
            columns::wrap(
                &format!(
                    "{kept} Voting never changes an outcome, and a withheld question never \
                     comes into Practice."
                ),
                width,
            )
            .into_iter()
            .map(|line| Line::styled(line, theme.muted)),
        );
        lines
    }

    /// Each check that withheld the candidate that shows, under the label
    /// Inspect uses, with Jev's answer and the learner's vote, drawn with
    /// `theme`, `width` cells wide.
    fn check_lines(&self, theme: &Theme, width: usize) -> Vec<Line<'static>> {
        let entry = self.current();
        let mut lines: Vec<Line<'static>> = columns::wrap(jev_answers::LABEL, width)
            .into_iter()
            .map(|line| Line::styled(line, theme.muted))
            .collect();
        for (place, &check) in entry.withheld.failed().iter().enumerate() {
            let (statement, banded) = self.banded(check);
            let lead = format!("{}. ", place + 1);
            // Measured in cells, as the lines are.
            let lead_width = Line::raw(lead.as_str()).width();
            let indent = " ".repeat(lead_width);
            for (index, text) in columns::wrap(&statement, width.saturating_sub(lead_width))
                .into_iter()
                .enumerate()
            {
                let lead = if index == 0 {
                    lead.clone()
                } else {
                    indent.clone()
                };
                lines.push(Line::raw(format!("{lead}{text}")));
            }
            let vote = match self.vote(check) {
                Some(VoteState::Saving(verdict)) => format!("{} · saving", verdict.label()),
                Some(VoteState::Saved(verdict)) => format!("{} · saved", verdict.label()),
                Some(VoteState::Unsaved(verdict, error)) => format!(
                    "{} · not saved: {error}. Vote again to save it.",
                    verdict.label()
                ),
                None if self.skipped.contains(&(self.at, check)) => "Skipped".to_owned(),
                None => "Not voted yet".to_owned(),
            };
            for text in [format!("Jev: {banded}"), format!("Your vote: {vote}")] {
                lines.extend(
                    columns::wrap(&text, width.saturating_sub(lead_width))
                        .into_iter()
                        .map(|text| Line::styled(format!("{indent}{text}"), theme.muted)),
                );
            }
        }
        lines
    }

    /// The lines heading every candidate: the label, and its place and area.
    fn head(&self, theme: &Theme) -> Vec<Line<'static>> {
        vec![
            Line::styled(WITHHELD, theme.title),
            Line::from(vec![
                Span::styled(
                    format!(
                        "Withheld question {} of {}",
                        self.at + 1,
                        self.candidates.len()
                    ),
                    theme.title,
                ),
                Span::styled(
                    format!(" · {} · {}", self.current().area_name, self.change.subject),
                    theme.muted,
                ),
            ]),
            Line::default(),
        ]
    }

    /// The controls, drawn with `theme` in rows `width` cells wide: the
    /// votes packed, then the others, each on its own row.
    fn control_rows(&self, theme: &Theme, width: usize) -> Vec<Line<'static>> {
        let focused = self.focused();
        let control = |control: Control| {
            theme.control(
                control.label(),
                ControlState {
                    focused: self.voting.is_none() && focused == control,
                    disabled: self.disabled(control),
                    ..ControlState::default()
                },
            )
        };
        let (votes, others): (Vec<Control>, Vec<Control>) = self
            .controls()
            .into_iter()
            .partition(|control| matches!(control, Control::Vote(_)));
        let mut rows = packed(votes.into_iter().map(control).collect(), width);
        rows.extend(others.into_iter().map(control));
        rows
    }
}

/// `controls`, each one line, packed in order into rows `width` cells wide,
/// two cells apart, so a row holds as many as fit.
fn packed(controls: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    for control in controls {
        match rows.last_mut() {
            Some(row) if row.width() + 2 + control.width() <= width => {
                row.push_span(Span::raw("  "));
                row.spans.extend(control.spans);
            }
            _ => rows.push(control),
        }
    }
    rows
}

/// Where the head, the text, a blank row and the `controls` rows go in
/// `area`. The controls keep their rows on a short screen, and the text
/// scrolls above them.
fn layout(head: usize, lines: usize, controls: usize, area: Rect) -> [Rect; 4] {
    let length = |rows: usize| u16::try_from(rows).unwrap_or(u16::MAX);
    Layout::vertical([
        Constraint::Length(length(head)),
        Constraint::Max(length(lines)),
        Constraint::Length(1),
        Constraint::Length(length(controls)),
    ])
    .areas(area)
}

impl View for ReviewWithheld {
    fn route(&self) -> Route {
        Route::Read(Reading::Withheld)
    }

    fn name(&self) -> &'static str {
        if self.voting.is_some() {
            "Vote"
        } else {
            "Review withheld questions"
        }
    }

    fn keymap(&self) -> Keymap {
        match &self.voting {
            Some((_, form)) => form.keymap(),
            None => KEYMAP,
        }
    }

    fn input(&self) -> Input {
        match &self.voting {
            Some((_, form)) => form.input(),
            None => Input::Commands,
        }
    }

    fn actions(&self) -> &'static [Action] {
        match &self.voting {
            Some(_) => vote::ACTIONS,
            None => ACTIONS,
        }
    }

    fn open(&self, route: Route, _context: &Context<'_>) -> Option<Box<dyn View>> {
        match route {
            Route::Read(Reading::Evidence) => Some(Box::new(self.evidence.clone())),
            _ => None,
        }
    }

    fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        for cast in self.to_save.drain(..) {
            let labels = Arc::clone(&self.labels);
            // The app applies the result to the labels file it keeps, so a
            // vote is saved even if the review closes first.
            jobs.start(move |_token| Ok(JobOutput::VoteSaved(Box::new(labels.write(&cast)))));
        }
    }

    fn paste(&mut self, text: &str) {
        if let Some((_, form)) = &mut self.voting {
            form.paste(text);
        }
    }

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        if let Some((place, form)) = &mut self.voting {
            let place = *place;
            let voting = form.apply(command);
            self.voted(place, voting);
            return None;
        }
        match command {
            Command::Activate => {
                let control = self.focused();
                if self.disabled(control).is_some() {
                    return None;
                }
                match control {
                    Control::Vote(place) => self.open_vote(place),
                    Control::InspectSource => {
                        return Some(Command::Go(Route::Read(Reading::Evidence)));
                    }
                    Control::NextWithheld => {
                        self.at = (self.at + 1) % self.candidates.len();
                        self.focus = Position::default();
                        self.top = Position::default();
                    }
                    Control::Back => return Some(Command::Back),
                }
            }
            Command::HalfPageDown | Command::HalfPageUp | Command::Top | Command::Bottom => {
                // Styles do not change how the text wraps.
                let theme = Theme::new(ColorSupport::NoColor);
                let lines = self.lines(&theme, area.width);
                let controls = self.control_rows(&theme, usize::from(area.width));
                let head = self.head(&theme).len();
                let [_, body, ..] = layout(head, lines.len(), controls.len(), area);
                let last = lines.len().saturating_sub(usize::from(body.height));
                self.top = self.top.scrolled(command, last, usize::from(body.height));
            }
            command => {
                self.focus = self.focus.after(command, self.controls().len());
            }
        }
        None
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let head = self.head(theme);
        let lines = self.lines(theme, area.width);
        let controls = self.control_rows(theme, usize::from(area.width));
        let [top, body, _, buttons] = layout(head.len(), lines.len(), controls.len(), area);
        frame.render_widget(Paragraph::new(head), top);
        let last = lines.len().saturating_sub(usize::from(body.height));
        let scrolled = self.top.index().min(last);
        frame.render_widget(
            Paragraph::new(lines).scroll((u16::try_from(scrolled).unwrap_or(u16::MAX), 0)),
            body,
        );
        frame.render_widget(List::new(controls), buttons);
        if let Some((_, form)) = &self.voting {
            form.render(theme, frame);
        }
    }
}
