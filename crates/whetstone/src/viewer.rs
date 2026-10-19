//! The change viewer: what a change does, file by file. The Files pane lists
//! the files the change touches, and beside it one code pane shows the
//! chosen file's diff, or its code before or after the change.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListState, Padding, Paragraph, Wrap};
use whetstone_core::{Area, Change, TouchedFile};

use crate::columns;
use crate::entry::TextEntry;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::palette::Action;
use crate::search::Query;
use crate::theme::{ControlKind, ControlState, Theme};
use crate::view::{Context, Route, View};

/// The viewer's own keys.
const KEYMAP: Keymap = &[
    keymap::PANES,
    Binding {
        does: "Scroll, or choose the next or previous file",
        ..keymap::MOVE
    },
    keymap::ENDS,
    keymap::HALF_PAGE,
    keymap::SEARCH,
    keymap::MATCHES,
    keymap::BACK,
];

/// The viewer's own actions in the palette.
const ACTIONS: &[Action] = &[
    Action {
        label: "Back",
        does: "Return to the picker",
        command: Command::Back,
    },
    Action {
        label: "Search",
        does: "Search the focused pane",
        command: Command::Search,
    },
    Action {
        label: "Next match",
        does: "Go to the next match of the last search",
        command: Command::NextMatch,
    },
    Action {
        label: "Previous match",
        does: "Go to the previous match of the last search",
        command: Command::PreviousMatch,
    },
];

/// The keys of the search box, while it has focus. Every other key but
/// Ctrl+C types.
const SEARCH_BOX: Keymap = &[
    Binding {
        keys: &[(Press::key(KeyCode::Enter), Command::Activate)],
        does: "Search for what was typed",
        hint: Some("Enter search"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Backspace), Command::Erase)],
        does: "Erase the last character",
        hint: None,
    },
    Binding {
        keys: &[(Press::key(KeyCode::Esc), Command::Cancel)],
        does: "Stop searching",
        hint: Some("Esc cancel"),
    },
];

/// What the code pane can show of the chosen file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Code {
    Diff,
    Before,
    After,
}

impl Code {
    const ALL: [Self; 3] = [Self::Diff, Self::Before, Self::After];

    fn label(self) -> &'static str {
        match self {
            Self::Diff => "Diff",
            Self::Before => "Before",
            Self::After => "After",
        }
    }

    /// Where it is in [`Code::ALL`].
    fn index(self) -> usize {
        match self {
            Self::Diff => 0,
            Self::Before => 1,
            Self::After => 2,
        }
    }
}

/// The viewer's panes. Only one code pane shows at a time, beside the Files
/// pane, so each has room for whole lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Files,
    Code(Code),
}

impl Pane {
    /// The panes from left to right.
    const ALL: [Self; 4] = [
        Self::Files,
        Self::Code(Code::Diff),
        Self::Code(Code::Before),
        Self::Code(Code::After),
    ];

    /// The pane `command` moves focus to from this one: `h`/`l` stop at
    /// either end, and Tab and Shift+Tab wrap around.
    fn after(self, command: Command) -> Self {
        let count = Self::ALL.len();
        let index = Self::ALL
            .iter()
            .position(|&pane| pane == self)
            .unwrap_or_default();
        Self::ALL[match command {
            Command::Left => index.saturating_sub(1),
            Command::Right => (index + 1).min(count - 1),
            Command::NextControl => (index + 1) % count,
            Command::PreviousControl => (index + count - 1) % count,
            _ => index,
        }]
    }
}

/// Where one code pane of one touched file is.
#[derive(Debug, Clone, Copy, Default)]
struct CodeView {
    /// The line at its top.
    top: Position,
    /// The line of the match a search last landed on, if one did.
    current_match: Option<usize>,
}

/// The viewer's state: which change it shows, which of the files it touches
/// is chosen, which pane has focus, and what the learner searched for.
#[derive(Debug, Clone)]
pub(crate) struct Viewer {
    /// The change's index among the showcase project's curated changes.
    index: usize,
    change: Change,
    /// The touched file the code pane shows.
    file: Position,
    focus: Pane,
    /// The code pane that shows: the focused one, or the one focused last.
    shown: Code,
    /// Where each code pane of each touched file is.
    views: Vec<[CodeView; 3]>,
    /// The search box, while the learner types a search.
    search_box: Option<TextEntry>,
    /// What the last search looked for.
    query: Option<Query>,
}

impl Viewer {
    /// The viewer on `change`, the showcase project's curated change at
    /// `index`, as it first opens: on the diff of the first file it touches.
    pub(crate) fn new(index: usize, change: &Change) -> Self {
        Self {
            index,
            change: change.clone(),
            file: Position::default(),
            focus: Pane::Code(Code::Diff),
            shown: Code::Diff,
            views: vec![[CodeView::default(); 3]; change.files().len()],
            search_box: None,
            query: None,
        }
    }

    /// Where the shown code pane is.
    fn view(&self) -> CodeView {
        self.views
            .get(self.file.index())
            .map(|views| views[self.shown.index()])
            .unwrap_or_default()
    }

    /// Where the shown code pane is, to change it.
    fn view_mut(&mut self) -> Option<&mut CodeView> {
        let shown = self.shown.index();
        self.views
            .get_mut(self.file.index())
            .map(|views| &mut views[shown])
    }

    /// How many lines the shown code pane has.
    fn line_count(&self) -> usize {
        self.code().map_or(1, |text| text.lines().count())
    }

    /// The furthest the shown code pane scrolls when it shows `rows` lines
    /// at a time: to where its last line is at the bottom.
    fn last_top(&self, rows: usize) -> usize {
        self.line_count().saturating_sub(rows)
    }

    /// Scrolls the shown code pane as `command` asks, when it shows `rows`
    /// lines at a time.
    fn scroll(&mut self, command: Command, rows: usize) {
        let last = self.last_top(rows);
        if let Some(view) = self.view_mut() {
            view.top = view.top.scrolled(command, last, rows);
        }
    }

    /// Searches the focused pane for `query`, from its top, or from its
    /// chosen file in the Files pane.
    fn search(&mut self, query: Query, rows: usize) {
        for views in &mut self.views {
            for view in views {
                view.current_match = None;
            }
        }
        let matches = self.matches(&query);
        self.query = Some(query);
        let from = match self.focus {
            Pane::Files => self.file.index(),
            Pane::Code(_) => self.view().top.index().min(self.last_top(rows)),
        };
        if let Some(&line) = matches
            .iter()
            .find(|&&line| line >= from)
            .or(matches.first())
        {
            self.land_on(line, rows);
        }
    }

    /// Moves to the next or previous match of the last search in the
    /// focused pane, as `command` asks, wrapping around at either end. It
    /// starts from the current match, or else from the top of a code pane,
    /// which shows `rows` lines at a time.
    fn step(&mut self, command: Command, rows: usize) {
        let Some(query) = &self.query else {
            return;
        };
        let matches = self.matches(query);
        let (current, top) = match self.focus {
            Pane::Files => (Some(self.file.index()), 0),
            Pane::Code(_) => (
                self.view().current_match,
                self.view().top.index().min(self.last_top(rows)),
            ),
        };
        let landing = match (command, current) {
            (Command::NextMatch, Some(current)) => matches
                .iter()
                .find(|&&line| line > current)
                .or(matches.first()),
            (Command::NextMatch, None) => matches
                .iter()
                .find(|&&line| line >= top)
                .or(matches.first()),
            (Command::PreviousMatch, current) => {
                let before = current.unwrap_or(top);
                matches
                    .iter()
                    .rev()
                    .find(|&&line| line < before)
                    .or(matches.last())
            }
            _ => None,
        };
        if let Some(&line) = landing {
            self.land_on(line, rows);
        }
    }

    /// The lines of the focused pane that hold `query`.
    fn matches(&self, query: &Query) -> Vec<usize> {
        match self.focus {
            Pane::Files => query.matching_lines(self.change.files().iter().map(TouchedFile::path)),
            Pane::Code(_) => self
                .code()
                .map(|text| query.matching_lines(text.lines()))
                .unwrap_or_default(),
        }
    }

    /// Makes the match on `line` of the focused pane the current one: in the
    /// Files pane its file is chosen. A code pane that shows `rows` lines at
    /// a time and does not show it scrolls it to the middle, as vim does, so
    /// the lines around it show too.
    fn land_on(&mut self, line: usize, rows: usize) {
        match self.focus {
            Pane::Files => self.file = Position::at(line),
            Pane::Code(_) => {
                let last = self.last_top(rows);
                if let Some(view) = self.view_mut() {
                    view.current_match = Some(line);
                    let top = view.top.index().min(last);
                    if !(top..top + rows).contains(&line) {
                        view.top = Position::at(line.saturating_sub(rows / 2).min(last));
                    }
                }
            }
        }
    }

    /// What the last search found in the focused pane.
    fn found(&self) -> Option<String> {
        let query = self.query.as_ref()?;
        let matches = self.matches(query);
        let current = match self.focus {
            Pane::Files => Some(self.file.index()),
            Pane::Code(_) => self.view().current_match,
        };
        let text = query.text();
        let pane = match self.focus {
            Pane::Files => "Files",
            Pane::Code(code) => code.label(),
        };
        Some(
            match current.and_then(|current| matches.iter().position(|&line| line == current)) {
                _ if matches.is_empty() => format!("No match for \"{text}\" in {pane}"),
                Some(at) => format!("Match {} of {} for \"{text}\"", at + 1, matches.len()),
                None if matches.len() == 1 => format!("1 match for \"{text}\""),
                None => format!("{} matches for \"{text}\"", matches.len()),
            },
        )
    }

    /// The chosen file.
    fn file(&self) -> Option<&TouchedFile> {
        self.change.files().get(self.file.index())
    }

    /// The code pane's text, or why there is none.
    fn code(&self) -> Result<&str, String> {
        let path = self.file().map(TouchedFile::path).unwrap_or_default();
        match self.shown {
            Code::Diff => Ok(self
                .change
                .file_diffs()
                .nth(self.file.index())
                .unwrap_or_default()),
            Code::Before => self
                .file()
                .and_then(TouchedFile::before)
                .ok_or_else(|| format!("This change creates {path}, so it has no code before.")),
            Code::After => self
                .file()
                .and_then(TouchedFile::after)
                .ok_or_else(|| format!("This change deletes {path}, so it has no code after.")),
        }
    }

    /// The `rows` lines of the code pane's `text` from the line at `top`:
    /// the diff, its header and hunk headers set apart, or code, numbered.
    /// What the last search looked for is marked, and the current match
    /// stands out.
    fn code_lines<'a>(
        &self,
        theme: &Theme,
        text: &'a str,
        top: usize,
        rows: usize,
    ) -> Vec<Line<'a>> {
        let digits = text.lines().count().to_string().len();
        let current_match = self.view().current_match;
        // A diff starts with its header lines, up to the first hunk.
        let header = text
            .lines()
            .position(|line| line.starts_with("@@"))
            .unwrap_or(usize::MAX);
        text.lines()
            .enumerate()
            .skip(top)
            .take(rows)
            .map(|(index, line)| {
                let style = match self.shown {
                    Code::Diff if index < header => theme.title,
                    Code::Diff if line.starts_with("@@") => theme.muted,
                    _ => Style::new(),
                };
                let mut spans = Vec::new();
                if self.shown != Code::Diff {
                    spans.push(Span::styled(
                        format!("{:>digits$} ", index + 1),
                        theme.muted,
                    ));
                }
                let found = if current_match == Some(index) {
                    theme.current_match
                } else {
                    theme.matched
                };
                spans.extend(marked(line, self.query.as_ref(), found));
                Line::from(spans).style(style)
            })
            .collect()
    }

    /// The code pane's title: each code pane, a choice, the shown one
    /// chosen.
    fn code_title(&self, theme: &Theme) -> Line<'static> {
        let mut title = Line::default();
        for code in Code::ALL {
            title.spans.extend(
                theme
                    .control(
                        code.label(),
                        ControlState {
                            focused: self.focus == Pane::Code(code),
                            kind: ControlKind::Choice {
                                selected: self.shown == code,
                            },
                            disabled: None,
                        },
                    )
                    .spans,
            );
            title.push_span(" ");
        }
        title
    }

    /// The touched files, each a choice.
    fn file_rows<'a>(&'a self, theme: &Theme) -> Vec<Line<'a>> {
        self.change
            .files()
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let chosen = index == self.file.index();
                theme.control(
                    file.path(),
                    ControlState {
                        focused: chosen && self.focus == Pane::Files,
                        kind: ControlKind::Choice { selected: chosen },
                        disabled: None,
                    },
                )
            })
            .collect()
    }
}

/// `line` in spans, with each occurrence of `query` styled `found`.
fn marked<'a>(line: &'a str, query: Option<&Query>, found: Style) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let mut at = 0;
    for occurrence in query
        .map(|query| query.occurrences(line))
        .unwrap_or_default()
    {
        if let (Some(before), Some(occurrence_text)) =
            (line.get(at..occurrence.start), line.get(occurrence.clone()))
        {
            spans.push(Span::raw(before));
            spans.push(Span::styled(occurrence_text, found));
            at = occurrence.end;
        }
    }
    spans.push(Span::raw(line.get(at..).unwrap_or_default()));
    spans
}

/// Where the viewer's parts go, top to bottom.
#[derive(Debug)]
struct Parts {
    title: Rect,
    panes: Rect,
    /// The row under the panes, for the search box and what a search found.
    status: Rect,
}

/// Where the viewer's parts go in `area`.
fn layout(area: Rect) -> Parts {
    let [title, _, panes, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    Parts {
        title,
        panes,
        status,
    }
}

/// Where the Files pane and the code pane go in `panes`, when the widest row
/// of the Files pane is `files_width` cells. The Files pane is as wide as its
/// rows, up to a third of the width, and the code pane takes the rest.
fn split(panes: Rect, files_width: usize) -> [Rect; 2] {
    // The rows, and the pane's border and a cell of padding on each side.
    let files_width = u16::try_from(files_width + 4).unwrap_or(u16::MAX);
    Layout::horizontal([
        Constraint::Length(files_width.min(panes.width / 3)),
        Constraint::Fill(1),
    ])
    .areas(panes)
}

/// How many lines a pane shows when the viewer is drawn in `area`: the
/// panes' height inside their borders.
fn pane_rows(area: Rect) -> usize {
    usize::from(layout(area).panes.height.saturating_sub(2))
}

/// The areas `change` touches, as the viewer's title shows them.
fn areas_of(change: &Change) -> String {
    change
        .areas()
        .iter()
        .map(Area::name)
        .collect::<Vec<_>>()
        .join(", ")
}

impl View for Viewer {
    fn route(&self) -> Route {
        Route::Viewer { change: self.index }
    }

    fn name(&self) -> &'static str {
        "Change"
    }

    fn keymap(&self) -> Keymap {
        match self.search_box {
            Some(_) => SEARCH_BOX,
            None => KEYMAP,
        }
    }

    fn actions(&self) -> &'static [Action] {
        ACTIONS
    }

    fn input(&self) -> Input {
        match self.search_box {
            Some(_) => Input::Text,
            None => Input::Commands,
        }
    }

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        if let Some(search_box) = &mut self.search_box {
            match command {
                Command::Activate => {
                    let query = Query::new(search_box.text());
                    self.search_box = None;
                    if let Some(query) = query {
                        self.search(query, pane_rows(area));
                    }
                }
                Command::Cancel => self.search_box = None,
                command => search_box.apply(command),
            }
            return None;
        }
        match command {
            Command::Search => {
                self.search_box = Some(TextEntry::default());
                return None;
            }
            Command::NextMatch | Command::PreviousMatch => {
                self.step(command, pane_rows(area));
                return None;
            }
            _ => {}
        }
        match self.focus {
            Pane::Files => {
                let last = self.change.files().len().saturating_sub(1);
                self.file = self.file.scrolled(command, last, pane_rows(area));
            }
            Pane::Code(_) => self.scroll(command, pane_rows(area)),
        }
        self.focus = self.focus.after(command);
        if let Pane::Code(code) = self.focus {
            self.shown = code;
        }
        None
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let file_rows = self.file_rows(theme);
        let parts = layout(area);
        let [files, code] = split(
            parts.panes,
            file_rows.iter().map(Line::width).max().unwrap_or_default(),
        );
        // The summary, and the areas the change touches right-aligned, at
        // least two cells after it.
        let mut title = Line::from(Span::styled(self.change.summary(), theme.title));
        let areas = areas_of(&self.change);
        let areas_column = usize::from(parts.title.width)
            .saturating_sub(Line::raw(&areas).width())
            .max(title.width() + 2);
        columns::pad_to(&mut title, areas_column);
        title.push_span(Span::styled(areas, theme.muted));
        frame.render_widget(Paragraph::new(title), parts.title);
        frame.render_stateful_widget(
            List::new(file_rows).block(
                Block::bordered()
                    .border_style(theme.border(self.focus == Pane::Files))
                    .title(Span::styled(" Files ", theme.title))
                    .padding(Padding::horizontal(1)),
            ),
            files,
            &mut ListState::default().with_selected(Some(self.file.index())),
        );
        // A window taller than the one it was scrolled in needs less
        // scrolling to show the end.
        let rows = pane_rows(area);
        let top = self.view().top.index().min(self.last_top(rows));
        let code_text = match self.code() {
            Ok(text) => Paragraph::new(self.code_lines(theme, text, top, rows)),
            // Unlike code, the reason wraps.
            Err(reason) => {
                Paragraph::new(Line::styled(reason, theme.muted)).wrap(Wrap { trim: true })
            }
        };
        frame.render_widget(
            code_text.block(
                Block::bordered()
                    .border_style(theme.border(matches!(self.focus, Pane::Code(_))))
                    .title(self.code_title(theme))
                    .padding(Padding::horizontal(1)),
            ),
            code,
        );
        if let Some(search_box) = &self.search_box {
            frame.render_widget(
                Paragraph::new(search_box.line(theme, "/", usize::from(parts.status.width))),
                parts.status,
            );
        } else if let Some(found) = self.found() {
            frame.render_widget(Paragraph::new(found), parts.status);
        }
    }
}
