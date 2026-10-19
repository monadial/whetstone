//! The change viewer: what a change does, file by file. The Files pane lists
//! the files the change touches, and beside it one code pane shows the
//! chosen file's diff, or its code before or after the change. Long lines
//! wrap, so every character shows at every supported size.

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
use crate::view::{ChangeIndex, Context, Route, View};

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
        label: "Next pane",
        does: "Move to the pane on the right, or back to Files",
        command: Command::NextControl,
    },
    Action {
        label: "Previous pane",
        does: "Move to the pane on the left, or round to After",
        command: Command::PreviousControl,
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
/// pane, so code has the width it needs.
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

/// What the code pane shows of the chosen file.
#[derive(Debug)]
enum Contents<'a> {
    /// The file's diff, or its code before or after the change.
    Text(&'a str),
    /// No code, because the change creates or deletes the file, and why.
    Missing(String),
}

/// How much of the code pane shows at once: its rows, and the cells across
/// each, inside its border and padding. Lines wider than that wrap.
#[derive(Debug, Clone, Copy)]
struct Page {
    rows: usize,
    width: u16,
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
    /// Where the change is in the showcase project's history.
    index: ChangeIndex,
    change: Change,
    /// The touched file the code pane shows.
    chosen_file: Position,
    focus: Pane,
    /// The code pane that shows: the focused one, or the one focused last.
    shown: Code,
    /// Where each code pane of each touched file is.
    views: Vec<[CodeView; Code::ALL.len()]>,
    /// The search box, while the learner types a search.
    search_box: Option<TextEntry>,
    /// What the last search looked for.
    query: Option<Query>,
}

impl Viewer {
    /// The viewer on `change`, the showcase project's curated change at
    /// `index`, as it first opens: on the diff of the first file it touches.
    pub(crate) fn new(index: ChangeIndex, change: &Change) -> Self {
        Self {
            index,
            change: change.clone(),
            chosen_file: Position::default(),
            focus: Pane::Code(Code::Diff),
            shown: Code::Diff,
            views: vec![[CodeView::default(); Code::ALL.len()]; change.files().len()],
            search_box: None,
            query: None,
        }
    }

    /// The chosen file.
    fn touched_file(&self) -> Option<&TouchedFile> {
        self.change.files().get(self.chosen_file.index())
    }

    /// What `code` shows of the chosen file.
    fn contents(&self, code: Code) -> Contents<'_> {
        let path = self
            .touched_file()
            .map(TouchedFile::path)
            .unwrap_or_default();
        let missing = |reason: String| move || Contents::Missing(reason);
        match code {
            Code::Diff => Contents::Text(
                self.change
                    .file_diffs()
                    .nth(self.chosen_file.index())
                    .unwrap_or_default(),
            ),
            Code::Before => self
                .touched_file()
                .and_then(TouchedFile::before)
                .map_or_else(
                    missing(format!(
                        "This change creates {path}, so it has no code before."
                    )),
                    Contents::Text,
                ),
            Code::After => self
                .touched_file()
                .and_then(TouchedFile::after)
                .map_or_else(
                    missing(format!(
                        "This change deletes {path}, so it has no code after."
                    )),
                    Contents::Text,
                ),
        }
    }

    /// The shown code pane's text, if it has any.
    fn text(&self) -> &str {
        match self.contents(self.shown) {
            Contents::Text(text) => text,
            Contents::Missing(_) => "",
        }
    }

    /// Where the shown code pane is.
    fn view(&self) -> CodeView {
        self.views
            .get(self.chosen_file.index())
            .map(|views| views[self.shown.index()])
            .unwrap_or_default()
    }

    /// Where the shown code pane is, to change it.
    fn view_mut(&mut self) -> Option<&mut CodeView> {
        let shown = self.shown.index();
        self.views
            .get_mut(self.chosen_file.index())
            .map(|views| &mut views[shown])
    }

    /// How many rows each line of the shown code pane takes on `page`,
    /// wrapped.
    fn heights(&self, page: Page) -> Vec<usize> {
        let text = self.text();
        let digits = digits(text);
        text.lines()
            .enumerate()
            .map(|(index, line)| {
                let gutter = self.gutter(index, digits).unwrap_or_default();
                let line = Line::raw(format!("{gutter}{line}"));
                // Only a line wider than the page wraps.
                if line.width() <= usize::from(page.width) {
                    1
                } else {
                    Paragraph::new(line)
                        .wrap(Wrap { trim: false })
                        .line_count(page.width)
                }
            })
            .collect()
    }

    /// The furthest the shown code pane scrolls on `page`: to where its
    /// last line is at the bottom.
    fn last_top(&self, page: Page) -> usize {
        let mut rows = 0;
        for (index, height) in self.heights(page).into_iter().enumerate().rev() {
            rows += height;
            if rows > page.rows {
                return index + 1;
            }
        }
        0
    }

    /// The line at the top of the shown code pane on `page`. A page taller
    /// than the one it was scrolled on needs less scrolling to show the
    /// end.
    fn top(&self, page: Page) -> usize {
        self.view().top.index().min(self.last_top(page))
    }

    /// Scrolls the shown code pane as `command` asks.
    fn scroll(&mut self, command: Command, page: Page) {
        let last = self.last_top(page);
        if let Some(view) = self.view_mut() {
            view.top = view.top.scrolled(command, last, page.rows);
        }
    }

    /// Where a search in the focused pane starts by default: the chosen
    /// file, or the top of a code pane.
    fn search_start(&self, page: Page) -> usize {
        match self.focus {
            Pane::Files => self.chosen_file.index(),
            Pane::Code(_) => self.top(page),
        }
    }

    /// Searches the focused pane for `query`, from where it is.
    fn search(&mut self, query: Query, page: Page) {
        for views in &mut self.views {
            for view in views {
                view.current_match = None;
            }
        }
        let matches = self.matches(&query);
        self.query = Some(query);
        if let Some(line) = first_from(&matches, self.search_start(page)) {
            self.land_on(line, page);
        }
    }

    /// Moves to the next or previous match of the last search in the
    /// focused pane, as `command` asks, wrapping around at either end. It
    /// moves on from the current match, or else from where a search starts.
    fn go_to_match(&mut self, command: Command, page: Page) {
        let Some(query) = &self.query else {
            return;
        };
        let matches = self.matches(query);
        let current = match self.focus {
            Pane::Files => Some(self.chosen_file.index()),
            Pane::Code(_) => self.view().current_match,
        };
        let start = self.search_start(page);
        let landing = match command {
            Command::NextMatch => first_from(&matches, current.map_or(start, |line| line + 1)),
            Command::PreviousMatch => {
                let before = current.unwrap_or(start);
                matches
                    .iter()
                    .rev()
                    .find(|&&line| line < before)
                    .or(matches.last())
                    .copied()
            }
            _ => None,
        };
        if let Some(line) = landing {
            self.land_on(line, page);
        }
    }

    /// The lines of the focused pane that hold `query`.
    fn matches(&self, query: &Query) -> Vec<usize> {
        match self.focus {
            Pane::Files => query.matching_lines(self.change.files().iter().map(TouchedFile::path)),
            Pane::Code(_) => query.matching_lines(self.text().lines()),
        }
    }

    /// Makes the match on `line` of the focused pane the current one: in the
    /// Files pane its file is chosen. A code pane that does not show it
    /// whole scrolls it to the middle, as vim does, so the lines around it
    /// show too.
    fn land_on(&mut self, line: usize, page: Page) {
        if self.focus == Pane::Files {
            self.chosen_file = Position::at(line);
            return;
        }
        let heights = self.heights(page);
        let top = self.top(page);
        let shown_whole = line >= top
            && heights
                .get(top..=line)
                .map_or(usize::MAX, |lines| lines.iter().sum())
                <= page.rows;
        let mut middle_top = line;
        let mut above = 0;
        while let Some(&height) = middle_top
            .checked_sub(1)
            .and_then(|before| heights.get(before))
        {
            if above + height > page.rows / 2 {
                break;
            }
            above += height;
            middle_top -= 1;
        }
        let middle_top = middle_top.min(self.last_top(page));
        if let Some(view) = self.view_mut() {
            view.current_match = Some(line);
            if !shown_whole {
                view.top = Position::at(middle_top);
            }
        }
    }

    /// What the last search found in the focused pane, for the row under
    /// the panes.
    fn search_status(&self) -> Option<String> {
        let query = self.query.as_ref()?;
        let matches = self.matches(query);
        let current = match self.focus {
            Pane::Files => Some(self.chosen_file.index()),
            Pane::Code(_) => self.view().current_match,
        };
        let text = query.text();
        let pane = match self.focus {
            Pane::Files => "Files",
            Pane::Code(code) => code.label(),
        };
        let at = current.and_then(|current| matches.iter().position(|&line| line == current));
        Some(match (matches.len(), at) {
            (0, _) => format!("No match for \"{text}\" in {pane}"),
            (count, Some(at)) => format!("Match {} of {count} for \"{text}\"", at + 1),
            (1, None) => format!("1 match for \"{text}\""),
            (count, None) => format!("{count} matches for \"{text}\""),
        })
    }

    /// The line number in front of the line at `index` of code whose last
    /// line number has `digits` digits. The diff has none.
    fn gutter(&self, index: usize, digits: usize) -> Option<String> {
        (self.shown != Code::Diff).then(|| format!("{:>digits$} ", index + 1))
    }

    /// The `rows` lines of `text` from the line at `top`: the diff, its
    /// header and hunk headers set apart, or code, numbered. What the last
    /// search looked for is marked, and the current match stands out.
    fn code_lines<'a>(
        &self,
        theme: &Theme,
        text: &'a str,
        top: usize,
        rows: usize,
    ) -> Vec<Line<'a>> {
        let digits = digits(text);
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
                let found = if current_match == Some(index) {
                    theme.current_match
                } else {
                    theme.matched
                };
                let mut spans: Vec<Span<'a>> = self
                    .gutter(index, digits)
                    .map(|gutter| Span::styled(gutter, theme.muted))
                    .into_iter()
                    .collect();
                spans.extend(marked(line, self.query.as_ref(), found));
                Line::from(spans).style(style)
            })
            .collect()
    }

    /// The code pane's title: each code pane, a choice, the shown one
    /// chosen. Before and After are disabled, saying why, for a file the
    /// change creates or deletes.
    fn code_title(&self, theme: &Theme) -> Line<'static> {
        let mut title = Line::default();
        for code in Code::ALL {
            let disabled = match (code, self.contents(code)) {
                (Code::Before, Contents::Missing(_)) => Some("new file"),
                (Code::After, Contents::Missing(_)) => Some("deleted"),
                _ => None,
            };
            title.spans.extend(
                theme
                    .control(
                        code.label(),
                        ControlState {
                            focused: self.focus == Pane::Code(code),
                            kind: ControlKind::Choice {
                                selected: self.shown == code,
                            },
                            disabled,
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
                let chosen = index == self.chosen_file.index();
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

    /// Where the viewer's parts go in `area`.
    fn layout(&self, area: Rect) -> Parts {
        let [title, _, panes, status] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);
        // The Files pane is as wide as its widest row, with a border and a
        // cell of padding on each side, up to a third of the width. The code
        // pane takes the rest.
        let files_width = self
            .change
            .files()
            .iter()
            .map(|file| Theme::control_width(file.path(), ControlKind::Choice { selected: true }))
            .max()
            .unwrap_or_default()
            + 4;
        let [files, code] = Layout::horizontal([
            Constraint::Length(
                u16::try_from(files_width)
                    .unwrap_or(u16::MAX)
                    .min(panes.width / 3),
            ),
            Constraint::Fill(1),
        ])
        .areas(panes);
        Parts {
            title,
            files,
            code,
            status,
        }
    }

    /// How much of the code pane shows at once when the viewer is drawn in
    /// `area`.
    fn page(&self, area: Rect) -> Page {
        let code = self.layout(area).code;
        Page {
            rows: usize::from(code.height.saturating_sub(2)),
            width: code.width.saturating_sub(4).max(1),
        }
    }
}

/// How many digits the last line number of `text` has.
fn digits(text: &str) -> usize {
    text.lines().count().to_string().len()
}

/// The first of `matches`, line numbers in order, at or after `start`,
/// wrapping around to the first.
fn first_from(matches: &[usize], start: usize) -> Option<usize> {
    matches
        .iter()
        .find(|&&line| line >= start)
        .or(matches.first())
        .copied()
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

/// Where the viewer's parts go.
#[derive(Debug)]
struct Parts {
    title: Rect,
    files: Rect,
    code: Rect,
    /// The row under the panes, for the search box and what a search found.
    status: Rect,
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
        Route::Viewer(self.index)
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
        let page = self.page(area);
        if let Some(search_box) = &mut self.search_box {
            match command {
                Command::Activate => {
                    let query = Query::new(search_box.text());
                    self.search_box = None;
                    if let Some(query) = query {
                        self.search(query, page);
                    }
                }
                Command::Cancel => self.search_box = None,
                command => search_box.apply(command),
            }
            return None;
        }
        match (command, self.focus) {
            (Command::Search, _) => self.search_box = Some(TextEntry::default()),
            (Command::NextMatch | Command::PreviousMatch, _) => self.go_to_match(command, page),
            (_, Pane::Files) => {
                let last = self.change.files().len().saturating_sub(1);
                self.chosen_file = self.chosen_file.scrolled(command, last, page.rows);
            }
            (_, Pane::Code(_)) => self.scroll(command, page),
        }
        self.focus = self.focus.after(command);
        if let Pane::Code(code) = self.focus {
            self.shown = code;
        }
        None
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let parts = self.layout(area);
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
            List::new(self.file_rows(theme)).block(
                Block::bordered()
                    .border_style(theme.border(self.focus == Pane::Files))
                    .title(Span::styled(" Files ", theme.title))
                    .padding(Padding::horizontal(1)),
            ),
            parts.files,
            &mut ListState::default().with_selected(Some(self.chosen_file.index())),
        );
        let page = self.page(area);
        let code = match self.contents(self.shown) {
            Contents::Text(text) => {
                Paragraph::new(self.code_lines(theme, text, self.top(page), page.rows))
            }
            Contents::Missing(reason) => Paragraph::new(Line::styled(reason, theme.muted)),
        };
        frame.render_widget(
            code.wrap(Wrap { trim: false }).block(
                Block::bordered()
                    .border_style(theme.border(matches!(self.focus, Pane::Code(_))))
                    .title(self.code_title(theme))
                    .padding(Padding::horizontal(1)),
            ),
            parts.code,
        );
        if let Some(search_box) = &self.search_box {
            frame.render_widget(
                Paragraph::new(search_box.line(theme, "/", usize::from(parts.status.width))),
                parts.status,
            );
        } else if let Some(status) = self.search_status() {
            frame.render_widget(Paragraph::new(status), parts.status);
        }
    }
}
