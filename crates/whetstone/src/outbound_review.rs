//! The outbound review: exactly what a Practice on the learner's repository
//! will send and where, before anything leaves the machine.
//!
//! It draws only from the change's [`Evidence`], and from the learner's
//! settings for where it goes. The learner leaves files, connected code and
//! related tests and configuration out, brings back what was left out by
//! default, and marks matches as not secrets; those review choices live in
//! memory, shared with the picker, which keeps them for this change until
//! another is chosen.
//!
//! Opening it starts a background job that finds the change's connected
//! code, with its related tests and related configuration, which have a
//! section of their own after it, once no other commit's analysis runs. The
//! review is usable meanwhile, and the evidence is built again when the
//! job's result arrives, if it is the result this review waits for, for
//! this commit (rule 24). The row of stages at its top shows finding
//! connected code as current until then. If the time limit runs out first,
//! the review carries on without connected code, and stays that way while
//! the job runs on and caches its graph for the commit's next review.
//!
//! Send, once an `OpenRouter` key was found, the evidence can be sent and
//! connected code is found or given up on, starts Practice in the review's
//! place, with exactly the evidence the review shows: what the learner
//! approves. Going back from Practice returns to the picker, which keeps the
//! choices for the change.

use std::cell::RefCell;
use std::mem;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use whetstone_core::{
    Area, AreaId, Change, CommitId, Comparison, ConnectedCodeLimit, ConnectedCodeLimitSource,
    ConnectionGroup, Connections, ConnectionsState, Evidence, EvidenceConnection, EvidenceFile,
    ExcludePatterns, Extent, FileIndex, Finding, FindingStatus, Origin, Place, RepositoryConfig,
    ReviewChoices, Severity, Side, Unsendable,
};

use crate::columns;
use crate::connected_code::{GraphOrigin, Request};
use crate::focus::Position;
use crate::job::{JobId, JobOutput, Jobs};
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::palette::Action;
use crate::practice::Practice;
use crate::reader::{Reader, Section};
use crate::stage_row::{PracticeStage, Stage, StageRow, StageState};
use crate::theme::{ColorSupport, ControlKind, ControlState, Theme};
use crate::view::{ChangeRef, Context, NEEDS_KEY, Reading, Route, View, comparison_label};

/// The review's own keys.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    Binding {
        keys: &[(Press::char(' '), Command::Activate)],
        does: "Leave out or bring back a file, connected code, a related test or configuration, \
               or resolve a possible secret",
        hint: Some("Space toggle"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Enter), Command::Open)],
        does: "Read what is sent for the change, a file, connected code, a related test or \
               configuration, or an area",
        hint: Some("Enter read"),
    },
    keymap::BACK,
];

/// The review's own action in the palette.
const ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Return to the picker",
    command: Command::Back,
}];

/// The screen's title.
const TITLE: &str = "Review what will be sent";
/// What each destination keeps of a request, and why: Jev requests can't
/// ask `OpenRouter` for zero-retention routing yet (#69), so the config
/// accepts only `TypeSafe` models for Jev, while generator requests ask for
/// it.
const GUARANTEES: [&str; 2] = [
    "Jev runs only on TypeSafe, whose endpoints keep no prompts.",
    "Generator requests ask OpenRouter only for endpoints that keep no prompts.",
];
/// `OpenRouter`'s privacy page.
const PRIVACY: &str = "https://openrouter.ai/docs/guides/privacy/data-collection";
/// What one approval covers.
const SCOPE: &str =
    "Your answers in this session go to the same places. Nothing outside this list is sent.";
/// What of the commit stays on the machine.
const NOT_SENT: &str = "Author, committer, dates and the commit ID are not sent.";
/// What the secret check cannot promise.
const DISCLAIMER: &str =
    "Whetstone checks for common secret formats and can't guarantee there are none.";
/// What a warning means, said once one shows.
const WARNINGS: &str = "Warnings don't block Send: their patterns often match what isn't a secret.";
/// Said over the limit.
const TOO_LARGE: &str = "This change is too large to practise on.";
/// Why Send waits while connected code is found: what the learner approves
/// is the evidence with it, or without it once it isn't available.
const FINDING: &str = "finding connected code";

/// In front of a line that belongs to the controls above it, as wide as the
/// focus marker.
const INDENT: &str = "  ";
/// In front of a row's relations when they go on the lines under it.
const RELATIONS_INDENT: &str = "    ";
/// How many of a connection's relations its row names before "+n more".
const RELATIONS_SHOWN: usize = 2;
/// What the size column shows for a file with no content.
const NO_SIZE: &str = "–";
/// Bytes in a kilobyte, as the limit counts them.
const KB: usize = 1000;

/// The review's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// The change, which Enter reads whole.
    Change,
    /// The touched file at this index.
    File(FileIndex),
    /// The connection at this index in the evidence.
    Connection(usize),
    /// The selected area at this index.
    Area(usize),
    /// Leaves out the file or connection of the finding at this index.
    LeaveOut(usize),
    /// Marks the finding at this index as not a secret.
    NotASecret(usize),
    Send,
    Back,
}

/// A section of the review that lists connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// Connected code.
    ConnectedCode,
    /// Related tests and related configuration.
    Related,
}

impl Listing {
    /// The groups of the connections it lists.
    fn groups(self) -> &'static [ConnectionGroup] {
        match self {
            Self::ConnectedCode => &[ConnectionGroup::ConnectedCode],
            Self::Related => &[
                ConnectionGroup::RelatedTest,
                ConnectionGroup::RelatedConfiguration,
            ],
        }
    }
}

/// The outbound review's state: the change and what it is built with, the
/// evidence they make, and where the learner is.
#[derive(Debug)]
pub(crate) struct OutboundReview {
    id: CommitId,
    /// Where it is sent, from the learner's settings.
    destination: String,
    change: Change,
    /// The selected areas.
    areas: Vec<Area>,
    /// The repository's exclude patterns.
    exclude: ExcludePatterns,
    /// The connected code limit that applies in the repository.
    connected_code_limit: ConnectedCodeLimit,
    /// The learner's choices for this change, shared with the picker.
    choices: Rc<RefCell<ReviewChoices>>,
    /// Where finding the change's connected code is.
    lookup: Lookup,
    /// How long the review waits for its connected code, from when it opens.
    time_limit: Duration,
    /// Whether Practice can ask Jev and the generator: whether an
    /// `OpenRouter` key was found.
    ai_available: bool,
    /// What the change, areas, patterns, connected code and choices make:
    /// exactly what Send sends.
    evidence: Evidence,
    focus: Position,
    /// The line at the top of the screen.
    top: usize,
}

/// Where finding a review's connected code is.
#[derive(Debug)]
enum Lookup {
    /// Asked for, until the review opens and starts the job that finds it.
    Requested(Box<Request>),
    /// Being found by the job with this identity, or waiting for its turn
    /// to be, which the review waits for until its time limit runs out.
    Running(JobId),
    /// Found, or why there is none, and how its graph was found.
    Done(Connections, GraphOrigin),
}

impl Lookup {
    /// What the evidence is built with: still finding, until it is done.
    fn connections(&self) -> &Connections {
        const FINDING: &Connections = &Connections::Finding;
        match self {
            Self::Requested(_) | Self::Running(_) => FINDING,
            Self::Done(connections, _) => connections,
        }
    }
}

/// The review laid out for a width: its lines, and where each control is.
#[derive(Debug, Default)]
struct Document<'a> {
    lines: Vec<Line<'a>>,
    /// For each control, in focus order, the lines to keep in view while it
    /// has focus: from its heading or what it acts on, down to its own row.
    controls: Vec<Range<usize>>,
}

impl<'a> Document<'a> {
    fn push(&mut self, line: impl Into<Line<'a>>) {
        self.lines.push(line.into());
    }

    fn blank(&mut self) {
        self.lines.push(Line::default());
    }

    /// Adds the control `line`, keeping the lines from `context_start` in
    /// view with it.
    fn control(&mut self, line: Line<'a>, context_start: usize) {
        self.controls.push(context_start..self.lines.len() + 1);
        self.lines.push(line);
    }

    /// Adds `text` wrapped to `width` cells, each line indented and styled
    /// with `style`.
    fn wrapped(&mut self, text: &str, width: usize, style: Style) {
        self.indented(INDENT, text, width, style);
    }

    /// Adds `text` wrapped to `width` cells, each line after `indent` and
    /// styled with `style`.
    fn indented(&mut self, indent: &'static str, text: &str, width: usize, style: Style) {
        let indent_width = Line::raw(indent).width();
        for line in columns::wrap(text, width.saturating_sub(indent_width)) {
            self.push(Line::from(vec![
                Span::raw(indent),
                Span::styled(line, style),
            ]));
        }
    }

    /// Keeps the lines added since the last control in view with it.
    fn extend_control(&mut self) {
        if let Some(control) = self.controls.last_mut() {
            control.end = self.lines.len();
        }
    }
}

impl OutboundReview {
    /// The review of the commit `id`, whose change is `change`, sent where
    /// the settings in `context` say, with the selected `areas`, leaving out
    /// what the repository's `config` leaves out, within the connected code
    /// limit that the settings and `config` make, as the learner's `choices`
    /// adjust it. Its connected code is found with `connected_code` once it
    /// opens, within the time limit in `context`.
    pub(crate) fn new(
        id: CommitId,
        context: &Context<'_>,
        change: Change,
        areas: Vec<Area>,
        config: &RepositoryConfig,
        choices: Rc<RefCell<ReviewChoices>>,
        connected_code: Request,
    ) -> Self {
        let settings = context.settings;
        let exclude = config.exclude().clone();
        let connected_code_limit = settings.connected_code_limit(config);
        let lookup = Lookup::Requested(Box::new(connected_code));
        let evidence = Evidence::build(
            &change,
            &exclude,
            &areas,
            lookup.connections(),
            connected_code_limit,
            &choices.borrow(),
        );
        tracing::info!(
            commit = id.full(),
            files = evidence.files().len(),
            sent = evidence
                .files()
                .iter()
                .filter(|file| file.sent().is_some())
                .count(),
            findings = evidence.findings().len(),
            bytes = evidence.size(),
            "opened the outbound review"
        );
        Self {
            id,
            destination: format!(
                "Sends to {} · Jev: {} · Generator: {}",
                settings.provider.value, settings.jev_model.value, settings.generator_model.value
            ),
            change,
            areas,
            exclude,
            connected_code_limit,
            choices,
            lookup,
            time_limit: context.time_limit,
            ai_available: context.ai.is_some(),
            evidence,
            focus: Position::default(),
            top: 0,
        }
    }

    /// The places among the evidence's connections of those `listing`
    /// lists, in order.
    fn listed(&self, listing: Listing) -> impl Iterator<Item = (usize, &EvidenceConnection)> {
        self.evidence
            .connections()
            .iter()
            .enumerate()
            .filter(move |(_, connection)| listing.groups().contains(&connection.group()))
    }

    /// Whether any connection `listing` lists was found, listed or not
    /// sent because it did not fit.
    fn found_any(&self, listing: Listing) -> bool {
        self.listed(listing).next().is_some()
            || !self.evidence.not_sent(listing.groups()).is_empty()
    }

    /// Applies `connections`, found for the commit `commit` from a graph
    /// found as `origin` says.
    fn apply_connections(
        &mut self,
        commit: CommitId,
        connections: Connections,
        origin: GraphOrigin,
    ) {
        if commit != self.id {
            return;
        }
        self.finish_lookup(connections, origin);
        tracing::info!(
            commit = self.id.full(),
            connections = self.evidence.connections().len(),
            sent = self
                .evidence
                .connections()
                .iter()
                .filter(|connection| connection.sent().is_some())
                .count(),
            findings = self.evidence.findings().len(),
            bytes = self.evidence.size(),
            "applied connected code to the outbound review"
        );
    }

    /// Ends finding connected code with `connections`, found from a graph
    /// found as `origin` says, or why there are none: the evidence is built
    /// again with them, so the size and the secret check cover them, and
    /// focus stays on the control it was on.
    fn finish_lookup(&mut self, connections: Connections, origin: GraphOrigin) {
        self.lookup = Lookup::Done(connections, origin);
        let focused = self.focused();
        self.rebuild();
        if let Some(at) = self
            .controls()
            .iter()
            .position(|control| *control == focused)
        {
            self.focus = Position::at(at);
        }
    }

    /// The controls, in focus order.
    fn controls(&self) -> Vec<Control> {
        let mut controls = vec![Control::Change];
        controls.extend(
            self.evidence
                .files()
                .iter()
                .map(|file| Control::File(file.index())),
        );
        for listing in [Listing::ConnectedCode, Listing::Related] {
            controls.extend(
                self.listed(listing)
                    .map(|(index, _)| Control::Connection(index)),
            );
        }
        controls.extend((0..self.evidence.areas().len()).map(Control::Area));
        for (index, finding) in self.evidence.findings().iter().enumerate() {
            if self.leave_out_label(finding.place()).is_some() {
                controls.push(Control::LeaveOut(index));
            }
            controls.push(Control::NotASecret(index));
        }
        controls.extend([Control::Send, Control::Back]);
        controls
    }

    fn focused(&self) -> Control {
        let controls = self.controls();
        controls
            .get(self.focus.index())
            .or(controls.last())
            .copied()
            .unwrap_or(Control::Back)
    }

    /// Builds the evidence again after a choice.
    fn rebuild(&mut self) {
        self.evidence = Evidence::build(
            &self.change,
            &self.exclude,
            &self.areas,
            self.lookup.connections(),
            self.connected_code_limit,
            &self.choices.borrow(),
        );
        let last = self.controls().len().saturating_sub(1);
        self.focus = Position::at(self.focus.index().min(last));
    }

    /// Applies the learner's choice on the focused control, if it makes
    /// one.
    fn choose(&mut self) {
        let mut choices = self.choices.borrow_mut();
        match self.focused() {
            Control::File(index) => choices.toggle_file(&self.evidence, index),
            Control::Connection(index) => {
                if let Some(connection) = self.evidence.connections().get(index) {
                    choices.toggle_connection(connection);
                }
            }
            Control::LeaveOut(index) => {
                let place = self.evidence.findings().get(index).map(Finding::place);
                if let Some(file) = place.and_then(Place::file) {
                    choices.leave_out(file);
                } else if let Some(connection) = place.and_then(Place::connection) {
                    choices.leave_out_connection(connection);
                }
            }
            Control::NotASecret(index) => {
                if let Some(finding) = self.evidence.findings().get(index) {
                    choices.toggle_not_a_secret(finding);
                }
            }
            Control::Change | Control::Area(_) | Control::Send | Control::Back => return,
        }
        drop(choices);
        self.rebuild();
    }

    /// Why Send is disabled, if it is: first that no `OpenRouter` key was
    /// found, so the learner knows before choosing anything, then the
    /// evidence's reason, which the learner can act on meanwhile, then that
    /// connected code is still being found.
    fn send_disabled(&self) -> Option<&'static str> {
        if !self.ai_available {
            return Some(NEEDS_KEY);
        }
        match self.evidence.unsendable() {
            Some(Unsendable::UnresolvedFindings) => Some("resolve the possible secrets"),
            Some(Unsendable::TooLarge) => Some("over the size limit"),
            Some(Unsendable::NothingToSend) => Some("nothing left to send"),
            None if self.evidence.connections_state() == ConnectionsState::Finding => Some(FINDING),
            None => None,
        }
    }

    /// Send, unless it is disabled: Practice on the change, in the review's
    /// place, with exactly the evidence the review shows.
    fn send(&self) -> Option<Command> {
        let practice = Route::Practice(ChangeRef::Commit(self.id));
        self.send_disabled()
            .is_none()
            .then_some(Command::Replace(practice))
    }

    /// How the control `control` shows its focus.
    fn state(&self, control: Control) -> ControlState<'static> {
        ControlState {
            focused: self.focused() == control,
            ..ControlState::default()
        }
    }

    /// The review laid out `width` cells wide, drawn with `theme`, its
    /// sections in order under the stage row, whose spinner is at its frame
    /// for `ticks`.
    fn document(&self, theme: &Theme, ticks: usize, width: u16) -> Document<'static> {
        let mut doc = Document::default();
        doc.lines.extend(self.stage_row(theme, ticks).lines(width));
        doc.blank();
        let width = usize::from(width);
        self.destination_rows(theme, width, &mut doc);
        self.change_rows(theme, width, &mut doc);
        let heading = doc.lines.len();
        doc.push(Line::styled("Files", theme.muted));
        self.file_rows(theme, width, &mut doc, heading);
        doc.blank();
        let summary = self.connected_code_summary();
        self.connection_rows(Listing::ConnectedCode, &summary, theme, width, &mut doc);
        doc.blank();
        let summary = self.related_summary();
        self.connection_rows(Listing::Related, &summary, theme, width, &mut doc);
        doc.blank();
        self.area_rows(theme, &mut doc);
        self.secret_check(theme, width, &mut doc);
        self.size_and_send(theme, width, &mut doc);
        doc
    }

    /// Practice's stages as the review stands, drawn with `theme`: the
    /// change is read, and connected code is found, or isn't available,
    /// before the evidence is prepared and the outbound review is where
    /// Practice is. The spinner moves with `ticks` only while connected code
    /// is found, and stands still on the outbound review.
    fn stage_row(&self, theme: &Theme, ticks: usize) -> StageRow<'static> {
        let finding_connected_code = match self.evidence.connections_state() {
            ConnectionsState::Finding => StageState::Current,
            ConnectionsState::Found | ConnectionsState::NoneFound => StageState::Done,
            ConnectionsState::TookTooLong | ConnectionsState::AnalysisFailed => StageState::Skipped,
        };
        let stages = PracticeStage::ALL.map(|stage| Stage {
            label: stage.label(),
            state: match stage {
                PracticeStage::FindingConnectedCode => finding_connected_code,
                PracticeStage::PreparingEvidence | PracticeStage::OutboundReview
                    if finding_connected_code == StageState::Current =>
                {
                    StageState::Upcoming
                }
                PracticeStage::ReadingTheChange | PracticeStage::PreparingEvidence => {
                    StageState::Done
                }
                PracticeStage::OutboundReview => StageState::Current,
                // Jev and the generator are asked once the learner sends.
                PracticeStage::CheckingAreas
                | PracticeStage::WritingQuestions
                | PracticeStage::CheckingQuestions => StageState::Upcoming,
            },
        });
        let ticks = if finding_connected_code == StageState::Current {
            ticks
        } else {
            0
        };
        StageRow::new(stages, ticks, theme)
    }

    /// The title, then where the evidence goes, what each destination keeps
    /// of it, and what one approval covers.
    fn destination_rows(&self, theme: &Theme, width: usize, doc: &mut Document<'static>) {
        doc.push(Line::styled(TITLE, theme.title));
        doc.blank();
        for text in [self.destination.as_str()].into_iter().chain(GUARANTEES) {
            doc.lines
                .extend(columns::wrap(text, width).into_iter().map(Line::raw));
        }
        doc.push(Line::styled(PRIVACY, theme.muted));
        doc.blank();
        doc.lines
            .extend(columns::wrap(SCOPE, width).into_iter().map(Line::raw));
        doc.blank();
    }

    /// The change: its short ID and summary, the summary wrapped under
    /// itself so that it shows whole, its message's other lines as they are
    /// sent, and what of the commit is not sent.
    fn change_rows(&self, theme: &Theme, width: usize, doc: &mut Document<'static>) {
        doc.push(Line::styled("Change", theme.muted));
        let mut lines = self.evidence.message().lines();
        let summary = lines.next().unwrap_or_default();
        let label = comparison_label(self.comparison());
        let short = self.id.short();
        // Where the summary starts, past the focus marker and the short ID.
        let lead =
            Theme::control_width("", ControlKind::Action) + Line::raw(short).width() + columns::GAP;
        let mut summary_lines = columns::wrap(
            summary,
            width
                .saturating_sub(lead)
                .saturating_sub(label.map_or(0, |label| columns::GAP + Line::raw(label).width())),
        )
        .into_iter();
        let mut row = theme.control(
            format!(
                "{short}{}{}",
                " ".repeat(columns::GAP),
                summary_lines.next().unwrap_or_default()
            ),
            self.state(Control::Change),
        );
        if let Some(label) = label {
            row.push_span(" ".repeat(columns::GAP));
            row.push_span(Span::styled(label, theme.muted));
        }
        // Focus on the change shows the review from the top.
        doc.control(row, 0);
        for line in summary_lines {
            doc.push(Line::raw(format!("{}{line}", " ".repeat(lead))));
        }
        for line in lines {
            if line.trim().is_empty() {
                doc.blank();
            } else {
                doc.wrapped(line, width, Style::new());
            }
        }
        doc.wrapped(NOT_SENT, width, theme.muted);
        doc.blank();
    }

    /// The selected areas' rows: each name, then where its prompt comes
    /// from.
    fn area_rows(&self, theme: &Theme, doc: &mut Document<'static>) {
        let heading = doc.lines.len();
        doc.push(Line::styled("Areas", theme.muted));
        let names = columns::next_column(self.evidence.areas().iter().map(Area::name));
        for (index, area) in self.evidence.areas().iter().enumerate() {
            let mut row = theme.control(area.name().to_owned(), self.state(Control::Area(index)));
            columns::pad_to(&mut row, Line::raw(INDENT).width() + names);
            row.push_span(Span::styled(area.source().to_string(), theme.muted));
            doc.control(row, if index == 0 { heading } else { doc.lines.len() });
        }
        doc.blank();
    }

    /// The secret check: each finding, with the actions that resolve it,
    /// then what a warning means, if one shows, and what the check cannot
    /// promise.
    fn secret_check(&self, theme: &Theme, width: usize, doc: &mut Document<'static>) {
        doc.push(Line::styled("Secret check", theme.muted));
        if self.evidence.findings().is_empty() {
            doc.wrapped("No possible secrets found.", width, Style::new());
        }
        for (index, finding) in self.evidence.findings().iter().enumerate() {
            let start = doc.lines.len();
            doc.wrapped(&self.describe(finding), width, Style::new());
            if let Some(label) = self.leave_out_label(finding.place()) {
                doc.control(
                    theme.control(label, self.state(Control::LeaveOut(index))),
                    start,
                );
            }
            doc.control(
                theme.control(
                    "Not a secret",
                    ControlState {
                        kind: ControlKind::Choice {
                            selected: finding.status() == FindingStatus::NotASecret,
                        },
                        ..self.state(Control::NotASecret(index))
                    },
                ),
                start,
            );
        }
        if self
            .evidence
            .findings()
            .iter()
            .any(|finding| finding.severity() == Severity::Warning)
        {
            doc.wrapped(WARNINGS, width, theme.muted);
        }
        doc.wrapped(DISCLAIMER, width, theme.muted);
        doc.blank();
    }

    /// The size against the limit, then Send, disabled with its reason,
    /// and Back.
    fn size_and_send(&self, theme: &Theme, width: usize, doc: &mut Document<'static>) {
        let heading = doc.lines.len();
        doc.push(Line::styled("Size", theme.muted));
        let size = format!(
            "{} of {}",
            kilobytes(self.evidence.size()),
            kilobytes(Evidence::SIZE_LIMIT)
        );
        doc.wrapped(&size, width, Style::new());
        if self.evidence.size() > Evidence::SIZE_LIMIT {
            doc.wrapped(TOO_LARGE, width, Style::new());
        }
        doc.blank();
        doc.control(
            theme.control(
                "Send",
                ControlState {
                    disabled: self.send_disabled(),
                    ..self.state(Control::Send)
                },
            ),
            heading,
        );
        let send = doc.lines.len() - 1;
        doc.control(theme.control("Back", self.state(Control::Back)), send);
    }

    /// The touched files' rows, `width` cells wide: each path, then its
    /// size and status in columns after the widest path, as far as the
    /// width allows. The Files heading is at line `heading`.
    fn file_rows(&self, theme: &Theme, width: usize, doc: &mut Document<'static>, heading: usize) {
        let files = self.evidence.files();
        if files.is_empty() {
            doc.wrapped("This change touches no files.", width, theme.muted);
            return;
        }
        let rows: Vec<(String, String, String)> = files
            .iter()
            .map(|file| {
                let path = match file.renamed_from() {
                    Some(from) => format!("{}  ← {from}", file.path()),
                    None => file.path().to_owned(),
                };
                // A submodule or a symbolic link has no content to size.
                let size = file.size().map_or_else(|| NO_SIZE.to_owned(), kilobytes);
                let status = match file.connected() {
                    0 => file.status().to_string(),
                    connected => format!("{} · {connected} connected", file.status()),
                };
                (path, size, status)
            })
            .collect();
        let size_width = rows
            .iter()
            .map(|(_, size, _)| Line::raw(size.as_str()).width())
            .max()
            .unwrap_or_default();
        let status_width = rows
            .iter()
            .map(|(_, _, status)| Line::raw(status.as_str()).width())
            .max()
            .unwrap_or_default();
        let marker = Theme::control_width("", ControlKind::Action);
        let path_width = rows
            .iter()
            .map(|(path, _, _)| Line::raw(path.as_str()).width())
            .max()
            .unwrap_or_default()
            .min(width.saturating_sub(marker + 2 * columns::GAP + size_width + status_width));
        for (index, (file, (path, size, status))) in files.iter().zip(rows).enumerate() {
            let focused = self.focused() == Control::File(file.index());
            let mut row = theme.control(
                columns::truncate(&path, path_width.max(1)),
                ControlState {
                    focused,
                    ..ControlState::default()
                },
            );
            columns::pad_to(&mut row, marker + path_width + columns::GAP);
            row.push_span(format!("{size:>size_width$}"));
            row.push_span(" ".repeat(columns::GAP));
            row.push_span(status);
            doc.control(row, if index == 0 { heading } else { doc.lines.len() });
        }
    }

    /// The section of what `listing` lists, `width` cells wide: its
    /// `summary`, then a row for each connection with its path, extent,
    /// size, status and relations in columns, then how many more are not
    /// sent and why. Relations that do not fit after the rest go on the
    /// lines under it.
    fn connection_rows(
        &self,
        listing: Listing,
        summary: &str,
        theme: &Theme,
        width: usize,
        doc: &mut Document<'static>,
    ) {
        let heading = doc.lines.len();
        doc.lines
            .extend(columns::wrap(summary, width).into_iter().map(Line::raw));
        let rows: Vec<(usize, [String; 4], String)> = self
            .listed(listing)
            .map(|(index, connection)| {
                let id = connection.id();
                (
                    index,
                    [
                        id.path().to_owned(),
                        id.extent().to_string(),
                        connection
                            .size()
                            .map_or_else(|| NO_SIZE.to_owned(), kilobytes),
                        connection.status().to_string(),
                    ],
                    relations(connection),
                )
            })
            .collect();
        let widest = |column: usize| {
            rows.iter()
                .map(|(_, cells, _)| Line::raw(cells[column].as_str()).width())
                .max()
                .unwrap_or_default()
        };
        let [extent_width, size_width, status_width] = [widest(1), widest(2), widest(3)];
        let marker = Theme::control_width("", ControlKind::Action);
        let path_width =
            widest(0).min(width.saturating_sub(
                marker + 3 * columns::GAP + extent_width + size_width + status_width,
            ));
        let listed = !rows.is_empty();
        for (row_index, (index, [path, extent, size, status], relations)) in
            rows.into_iter().enumerate()
        {
            let mut row = theme.control(
                columns::truncate(&path, path_width.max(1)),
                self.state(Control::Connection(index)),
            );
            let extent_at = marker + path_width + columns::GAP;
            let size_at = extent_at + extent_width + columns::GAP;
            let status_at = size_at + size_width + columns::GAP;
            let relations_at = status_at + status_width + columns::GAP;
            columns::pad_to(&mut row, extent_at);
            row.push_span(extent);
            columns::pad_to(&mut row, size_at);
            row.push_span(format!("{size:>size_width$}"));
            columns::pad_to(&mut row, status_at);
            row.push_span(status);
            let fits = relations_at + Line::raw(relations.as_str()).width() <= width;
            if fits {
                columns::pad_to(&mut row, relations_at);
                row.push_span(relations.clone());
            }
            doc.control(
                row,
                if row_index == 0 {
                    heading
                } else {
                    doc.lines.len()
                },
            );
            if !fits {
                doc.indented(RELATIONS_INDENT, &relations, width, Style::new());
                doc.extend_control();
            }
        }
        for (reason, count) in self.evidence.not_sent(listing.groups()) {
            doc.wrapped(
                &format!("{count} more not sent: {reason}"),
                width,
                Style::new(),
            );
        }
        // The last row keeps what was not sent in view, as the first keeps
        // the summary.
        if listed {
            doc.extend_control();
        }
    }

    /// The Connected code section's summary: still finding it, how much is
    /// sent and of what limit, and whether it came from the cache; none
    /// found; or why it is not available.
    fn connected_code_summary(&self) -> String {
        match self.evidence.connections_state() {
            ConnectionsState::Finding => "Connected code: finding…".to_owned(),
            ConnectionsState::Found if self.found_any(Listing::ConnectedCode) => {
                let sent: Vec<&EvidenceConnection> = self
                    .listed(Listing::ConnectedCode)
                    .map(|(_, connection)| connection)
                    .filter(|connection| connection.sent().is_some())
                    .collect();
                let ranges = sent
                    .iter()
                    .filter(|connection| matches!(connection.id().extent(), Extent::Lines(_)))
                    .count();
                let counted = match (ranges, sent.len() - ranges) {
                    (0, 0) => "none sent".to_owned(),
                    (ranges, 0) => counted(ranges, "range"),
                    (0, files) => counted(files, "file"),
                    (ranges, files) => {
                        format!(
                            "{} and {}",
                            counted(ranges, "range"),
                            counted(files, "file")
                        )
                    }
                };
                let limit = self.evidence.connected_code_limit();
                let cached = match self.lookup {
                    Lookup::Done(_, GraphOrigin::Cached) => " · from cache",
                    _ => "",
                };
                let lowered = match limit.source() {
                    ConnectedCodeLimitSource::LoweredByRepository => {
                        " · limit lowered by this repository"
                    }
                    ConnectedCodeLimitSource::Default | ConnectedCodeLimitSource::Learner => "",
                };
                format!(
                    "Connected code: {counted} · {} of {} limit{cached}{lowered}",
                    kilobytes(self.evidence.connected_code_size()),
                    limit.kb()
                )
            }
            ConnectionsState::Found | ConnectionsState::NoneFound => {
                "Connected code: none found for this change.".to_owned()
            }
            ConnectionsState::TookTooLong => format!(
                "Connected code isn't available for this change: finding it took longer than {} s.",
                self.time_limit.as_secs()
            ),
            ConnectionsState::AnalysisFailed => {
                "Connected code isn't available for this change: the analysis failed.".to_owned()
            }
        }
    }

    /// The Related tests and configuration section's summary: still
    /// finding them, how many tests and manifests are sent and their size,
    /// none found, or why they are not available, which is why connected
    /// code is not.
    fn related_summary(&self) -> String {
        const RELATED: &str = "Related tests and configuration";
        match self.evidence.connections_state() {
            ConnectionsState::Finding => format!("{RELATED}: finding…"),
            ConnectionsState::Found if self.found_any(Listing::Related) => {
                let sent: Vec<&EvidenceConnection> = self
                    .listed(Listing::Related)
                    .map(|(_, connection)| connection)
                    .filter(|connection| connection.sent().is_some())
                    .collect();
                let tests = sent
                    .iter()
                    .filter(|connection| connection.group() == ConnectionGroup::RelatedTest)
                    .count();
                let counted = match (tests, sent.len() - tests) {
                    (0, 0) => "none sent".to_owned(),
                    (tests, 0) => counted(tests, "test"),
                    (0, manifests) => counted(manifests, "manifest"),
                    (tests, manifests) => {
                        format!(
                            "{} and {}",
                            counted(tests, "test"),
                            counted(manifests, "manifest")
                        )
                    }
                };
                let size = sent.iter().filter_map(|connection| connection.size()).sum();
                format!("{RELATED}: {counted} · {}", kilobytes(size))
            }
            ConnectionsState::Found | ConnectionsState::NoneFound => {
                format!("{RELATED}: none found for this change.")
            }
            ConnectionsState::TookTooLong => format!(
                "{RELATED} aren't available for this change: finding them took longer than {} s.",
                self.time_limit.as_secs()
            ),
            ConnectionsState::AnalysisFailed => {
                format!("{RELATED} aren't available for this change: the analysis failed.")
            }
        }
    }

    /// The control that resolves a finding at `place` by leaving out what
    /// it is in, if that can be left out: a file, connected code, a related
    /// test or related configuration can be, the commit message and an area,
    /// which the learner chose before the review, cannot.
    fn leave_out_label(&self, place: &Place) -> Option<&'static str> {
        match place {
            Place::CommitMessage
            | Place::AreaName(_)
            | Place::AreaDescription(_)
            | Place::AreaPrompt(_) => None,
            Place::Content(_) | Place::Diff(_) => Some("Leave out the file"),
            Place::Connection(id) => Some(
                match self
                    .evidence
                    .connections()
                    .iter()
                    .find(|connection| connection.id() == id)
                    .map(EvidenceConnection::group)
                {
                    Some(ConnectionGroup::RelatedTest) => "Leave out the related test",
                    Some(ConnectionGroup::RelatedConfiguration) => {
                        "Leave out the related configuration"
                    }
                    Some(ConnectionGroup::ConnectedCode) | None => "Leave out the connected code",
                },
            ),
        }
    }

    /// A finding as one sentence: whether it is only a warning, where it
    /// is, the rule that matched and the masked excerpt. An area's name and
    /// description are one line each, so a finding in either names no line.
    fn describe(&self, finding: &Finding) -> String {
        let matched = finding.matched();
        let line = matched.line();
        let place = match finding.place() {
            Place::CommitMessage => format!("the commit message, line {line}"),
            Place::Content(file) => format!("{}, line {line}", self.path(*file)),
            Place::Diff(file) => format!("{}, diff line {line}", self.path(*file)),
            Place::Connection(connection) => format!("{}, line {line}", connection.path()),
            Place::AreaName(area) => format!("the {} area's name", self.area_name(area)),
            Place::AreaDescription(area) => {
                format!("the {} area's description", self.area_name(area))
            }
            Place::AreaPrompt(area) => {
                format!("the {} area's prompt, line {line}", self.area_name(area))
            }
        };
        let lead = match finding.severity() {
            Severity::Blocking => "Possible secret",
            Severity::Warning => "Warning: possible secret",
        };
        format!(
            "{lead} in {place}: {}, {}",
            matched.rule().name(),
            matched.excerpt()
        )
    }

    /// The name of the selected area `id` names.
    fn area_name(&self, id: &AreaId) -> &str {
        self.evidence
            .areas()
            .iter()
            .find(|area| area.id() == id)
            .map(Area::name)
            .unwrap_or_default()
    }

    /// The path of the touched file at `index`.
    fn path(&self, index: FileIndex) -> &str {
        self.evidence
            .file(index)
            .map(EvidenceFile::path)
            .unwrap_or_default()
    }

    /// What the change was compared with.
    fn comparison(&self) -> Comparison {
        match self.change.origin() {
            Origin::Commit { comparison, .. } => *comparison,
            Origin::Showcase { .. } => Comparison::Parent,
        }
    }

    /// The line at the top of `doc` in `area`: where it was, moved only as
    /// far as it must to show the focused control and what it acts on.
    fn scrolled(&self, doc: &Document<'_>, area: Rect) -> usize {
        let shown = doc
            .controls
            .get(self.focus.index())
            .cloned()
            .unwrap_or_default();
        scrolled_top(self.top, shown, usize::from(area.height), doc.lines.len())
    }

    /// The reader for `reading`.
    fn reader(&self, reading: Reading) -> Option<Reader> {
        let route = Route::Read(reading);
        Some(match reading {
            Reading::Change => Reader::new(
                route,
                format!("Change {}", self.id.short()),
                vec![
                    Section::plain("Commit ID, which is not sent", self.id.full()),
                    Section::plain("Message, as it is sent", self.evidence.message()),
                ],
            ),
            Reading::File(index) => {
                let file = self.evidence.file(index)?;
                let title = format!("What is sent for {}", file.path());
                match file.sent() {
                    Some(sent) => Reader::new(
                        route,
                        title,
                        vec![
                            Section::numbered("Diff", sent.diff),
                            Section::numbered(
                                match sent.side {
                                    Side::After => "Full content",
                                    Side::Before => {
                                        "Full content before the change, which deletes it"
                                    }
                                },
                                sent.content,
                            ),
                        ],
                    ),
                    None => Reader::new(
                        route,
                        title,
                        vec![Section::plain(
                            "Nothing is sent",
                            &format!("{} is {}.", file.path(), file.status()),
                        )],
                    ),
                }
            }
            Reading::Connection(index) => {
                let connection = self.evidence.connections().get(index)?;
                let id = connection.id();
                let title = format!("What is sent for {}, {}", id.path(), id.extent());
                let why = connection
                    .relations()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n");
                let sent = match connection.sent() {
                    Some(sent) => {
                        Section::numbered_from("Content, as it is sent", sent.text, sent.first_line)
                    }
                    None => Section::plain(
                        "Nothing is sent",
                        &format!(
                            "{}, {}, is {}.",
                            id.path(),
                            id.extent(),
                            connection.status()
                        ),
                    ),
                };
                Reader::new(
                    route,
                    title,
                    vec![Section::plain("Why it is sent", &why), sent],
                )
            }
            Reading::Area(index) => {
                let area = self.evidence.areas().get(index)?;
                // Numbered, as the secret check cites a line in the prompt.
                Reader::new(
                    route,
                    format!("What is sent for the {} area", area.name()),
                    vec![
                        Section::plain("Prompt source", &area.source().to_string()),
                        Section::plain("Description, as it is sent", area.description()),
                        Section::numbered("Area prompt, as it is sent", area.prompt()),
                    ],
                )
            }
            // Only Practice's questions show it.
            Reading::Evidence
            | Reading::Answer
            | Reading::JevAnswers
            | Reading::WhyWithheld
            | Reading::Withheld => {
                return None;
            }
        })
    }
}

/// `count` of `noun`, as "1 range" or "3 ranges".
fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        count => format!("{count} {noun}s"),
    }
}

/// What a connection's row says it connects to: its first relations, then
/// how many more there are.
fn relations(connection: &EvidenceConnection) -> String {
    let relations = connection.relations();
    let mut shown: Vec<String> = relations
        .iter()
        .take(RELATIONS_SHOWN)
        .map(ToString::to_string)
        .collect();
    if relations.len() > RELATIONS_SHOWN {
        shown.push(format!("+{} more", relations.len() - RELATIONS_SHOWN));
    }
    shown.join(", ")
}

/// `bytes` in kilobytes, rounded up, so that anything over the limit shows
/// over it.
fn kilobytes(bytes: usize) -> String {
    format!("{} KB", bytes.div_ceil(KB))
}

/// The line at the top of a screen `rows` high showing `total` lines, when
/// it was `top` and `shown` must be in view: it moves only as far as it
/// must, and keeps the end of `shown` in view if it cannot show it all.
fn scrolled_top(top: usize, shown: Range<usize>, rows: usize, total: usize) -> usize {
    let mut top = top.min(total.saturating_sub(rows));
    if shown.start < top {
        top = shown.start;
    }
    if shown.end > top + rows {
        top = shown.end.saturating_sub(rows);
    }
    top
}

impl View for OutboundReview {
    fn route(&self) -> Route {
        Route::OutboundReview(self.id)
    }

    fn name(&self) -> &'static str {
        "Outbound review"
    }

    fn keymap(&self) -> Keymap {
        KEYMAP
    }

    fn actions(&self) -> &'static [Action] {
        ACTIONS
    }

    fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        self.lookup = match mem::replace(
            &mut self.lookup,
            Lookup::Done(Connections::Finding, GraphOrigin::Analysed),
        ) {
            Lookup::Requested(request) => {
                // The engine can't be stopped once it starts, and two
                // analyses would compete for the machine, so they take
                // turns, and only the newest commit's waits.
                let job = jobs.start_one_at_a_time(move |_token| {
                    let (connections, origin) = request.find();
                    Ok(JobOutput::ConnectedCode {
                        commit: request.commit(),
                        connections,
                        origin,
                    })
                });
                // From now, as the review opens, whether the job runs at
                // once or waits for its turn.
                jobs.limit(job, self.time_limit);
                Lookup::Running(job)
            }
            lookup => lookup,
        };
    }

    fn waits_for(&self, job: JobId) -> bool {
        matches!(self.lookup, Lookup::Running(waiting) if waiting == job)
    }

    fn finish_job(&mut self, job: JobId, output: JobOutput) {
        if !self.waits_for(job) {
            return;
        }
        match output {
            JobOutput::ConnectedCode {
                commit,
                connections,
                origin,
            } => self.apply_connections(commit, connections, origin),
            // Only Practice waits for these.
            JobOutput::ShowcaseConnectedCode { .. }
            | JobOutput::AreasChecked { .. }
            | JobOutput::Written { .. }
            | JobOutput::Checked { .. }
            | JobOutput::Evaluated { .. }
            | JobOutput::Explained { .. }
            | JobOutput::ExplanationChecked { .. }
            | JobOutput::VoteSaved(_)
            | JobOutput::CommitsRead(_)
            | JobOutput::ChangeSized { .. } => {}
        }
    }

    fn time_up(&mut self, job: JobId) {
        if !self.waits_for(job) {
            return;
        }
        // No graph came, so its origin is `Analysed`, as for any result
        // without one.
        self.finish_lookup(Connections::TookTooLong, GraphOrigin::Analysed);
        tracing::info!(
            commit = self.id.full(),
            limit_s = self.time_limit.as_secs(),
            "gave up finding connected code for the outbound review"
        );
    }

    fn open(&self, route: Route, context: &Context<'_>) -> Option<Box<dyn View>> {
        match route {
            Route::Read(reading) => self
                .reader(reading)
                .map(|reader| Box::new(reader) as Box<dyn View>),
            Route::Practice(ChangeRef::Commit(id))
                if id == self.id && self.send_disabled().is_none() =>
            {
                Practice::on_repository(id, self.change.clone(), self.evidence.clone(), context)
                    .map(|practice| Box::new(practice) as Box<dyn View>)
            }
            _ => None,
        }
    }

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        let next = match command {
            Command::Activate | Command::Open => match self.focused() {
                Control::Back => Some(Command::Back),
                Control::Send => self.send(),
                Control::Change if command == Command::Open => {
                    Some(Command::Go(Route::Read(Reading::Change)))
                }
                Control::File(index) if command == Command::Open => {
                    Some(Command::Go(Route::Read(Reading::File(index))))
                }
                Control::Area(index) if command == Command::Open => {
                    Some(Command::Go(Route::Read(Reading::Area(index))))
                }
                Control::Connection(index) if command == Command::Open => {
                    Some(Command::Go(Route::Read(Reading::Connection(index))))
                }
                _ => {
                    self.choose();
                    None
                }
            },
            command => {
                self.focus = self.focus.after(command, self.controls().len());
                None
            }
        };
        // Styles and the spinner's frame do not change the layout.
        let doc = self.document(&Theme::new(ColorSupport::NoColor), 0, area.width);
        self.top = self.scrolled(&doc, area);
        next
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let doc = self.document(context.theme, context.ticks, area.width);
        let top = self.scrolled(&doc, area);
        let lines: Vec<Line<'_>> = doc
            .lines
            .into_iter()
            .skip(top)
            .take(usize::from(area.height))
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }
}
