//! The change viewer: what a change does, file by file. The Files pane lists
//! the files the change touches, and beside it one code pane shows the
//! chosen file's diff, or its code before or after the change. Long lines
//! wrap, so every character shows at every supported size.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, List, ListState, Padding, Paragraph, Wrap};
use whetstone_core::{Change, CommitId, FileContent, Origin, TouchedFile};

use crate::columns;
use crate::entry::TextEntry;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap};
use crate::palette::Action;
use crate::repository::LoadError;
use crate::search::{self, Query};
use crate::theme::{ControlKind, ControlState, Theme};
use crate::view::{ChangeRef, Context, Route, View, comparison_label};

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
    /// No code, because the change creates or deletes the file, or that
    /// side of it is not text.
    Missing {
        /// Why, in a sentence.
        reason: String,
        /// Why the pane is disabled in its title, if it is: only for a file
        /// the change creates or deletes. A side that is not text is named
        /// in the Files pane, and the title has no room to say so too.
        disabled: Option<&'static str>,
    },
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
    /// Which change it is.
    at: ChangeRef,
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
    /// The viewer on `change`, the one `at` names, as it first opens: on
    /// the diff of the first file it touches.
    pub(crate) fn new(at: ChangeRef, change: Change) -> Self {
        Self {
            at,
            views: vec![[CodeView::default(); Code::ALL.len()]; change.files().len()],
            change,
            chosen_file: Position::default(),
            focus: Pane::Code(Code::Diff),
            shown: Code::Diff,
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
        let file = self.touched_file();
        let path = file.map(TouchedFile::path).unwrap_or_default();
        let missing = |reason: String, disabled| Contents::Missing {
            reason,
            disabled: Some(disabled),
        };
        match code {
            Code::Diff => Contents::Text(
                self.change
                    .file_diffs()
                    .nth(self.chosen_file.index())
                    .unwrap_or_default(),
            ),
            Code::Before => match file.and_then(TouchedFile::before) {
                None => missing(
                    format!("This change creates {path}, so it has no code before."),
                    "new file",
                ),
                Some(content) => text_or(content, |kind| {
                    let path = file.and_then(TouchedFile::renamed_from).unwrap_or(path);
                    format!("{path} was {kind} before this change, so it has no code to show.")
                }),
            },
            Code::After => match file.and_then(TouchedFile::after) {
                None => missing(
                    format!("This change deletes {path}, so it has no code after."),
                    "deleted",
                ),
                Some(content) => text_or(content, |kind| {
                    format!("{path} is {kind}, so it has no code to show.")
                }),
            },
        }
    }

    /// The shown code pane's text, if it has any.
    fn text(&self) -> &str {
        match self.contents(self.shown) {
            Contents::Text(text) => text,
            Contents::Missing { .. } => "",
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
        if let Some(line) = search::first_from(&matches, self.search_start(page)) {
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
            Command::NextMatch => {
                search::first_from(&matches, current.map_or(start, |line| line + 1))
            }
            Command::PreviousMatch => search::last_before(&matches, current.unwrap_or(start)),
            _ => None,
        };
        if let Some(line) = landing {
            self.land_on(line, page);
        }
    }

    /// The lines of the focused pane that hold `query`.
    fn matches(&self, query: &Query) -> Vec<usize> {
        match self.focus {
            Pane::Files => {
                let names: Vec<String> = self.change.files().iter().map(file_text).collect();
                query.matching_lines(names.iter().map(String::as_str))
            }
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
        let pane = match self.focus {
            Pane::Files => "Files",
            Pane::Code(code) => code.label(),
        };
        Some(search::status(query, &matches, current, pane))
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
    /// change creates or deletes, or a side that is not text.
    fn code_title(&self, theme: &Theme) -> Line<'static> {
        let mut title = Line::default();
        for code in Code::ALL {
            let disabled = match self.contents(code) {
                Contents::Missing { disabled, .. } => disabled,
                Contents::Text(_) => None,
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

    /// The touched files as items `width` cells wide, each a choice
    /// showing its path, then where a rename moved it from and what it
    /// holds if that is not text. When those do not fit after the path,
    /// they go on a line of their own under it, so the path keeps the row.
    fn file_rows<'a>(&'a self, theme: &Theme, width: u16) -> Vec<Text<'a>> {
        self.change
            .files()
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let chosen = index == self.chosen_file.index();
                let state = ControlState {
                    focused: chosen && self.focus == Pane::Files,
                    kind: ControlKind::Choice { selected: chosen },
                    disabled: None,
                };
                let mut row = theme.control(file.path(), state);
                let details = file_details(file, theme.muted);
                if details.spans.is_empty() {
                    return Text::from(row);
                }
                if row.width() + columns::GAP + details.width() <= usize::from(width) {
                    row.push_span(" ".repeat(columns::GAP));
                    row.spans.extend(details.spans);
                    return Text::from(row);
                }
                let mut below = Line::raw(" ".repeat(Theme::control_width("", state.kind)));
                below.spans.extend(details.spans);
                Text::from(vec![row, below])
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
            .map(|file| {
                let details = file_details(file, Style::new()).width();
                Theme::control_width(file.path(), ControlKind::Choice { selected: true })
                    + if details > 0 {
                        columns::GAP + details
                    } else {
                        0
                    }
            })
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

/// What the Files pane shows of a touched file, as one line of text for a
/// search to look through: its path, then its details.
fn file_text(file: &TouchedFile) -> String {
    let mut text = file.path().to_owned();
    let details = file_details(file, Style::new());
    if !details.spans.is_empty() {
        text.push_str(&" ".repeat(columns::GAP));
        text.extend(details.spans.iter().map(|span| span.content.as_ref()));
    }
    text
}

/// What the Files pane says of a touched file after its path, if anything:
/// where a rename moved it from, and, styled `muted`, what it holds after
/// the change, or before it if the change deletes it, if that is not text.
fn file_details(file: &TouchedFile, muted: Style) -> Line<'_> {
    let mut details = Line::default();
    if let Some(from) = file.renamed_from() {
        details.push_span(format!("← {from}"));
    }
    let kind = file
        .after()
        .or_else(|| file.before())
        .filter(|content| content.text().is_none())
        .map(|content| Kind::of(content).word);
    if let Some(kind) = kind {
        if !details.spans.is_empty() {
            details.push_span(" ".repeat(columns::GAP));
        }
        details.push_span(Span::styled(kind, muted));
    }
    details
}

/// What a code pane shows of `content`: its text, or why it has none, in a
/// sentence that `sentence` makes from what it is.
fn text_or(content: &FileContent, sentence: impl FnOnce(&str) -> String) -> Contents<'_> {
    if let FileContent::Text(text) = content {
        return Contents::Text(text);
    }
    Contents::Missing {
        reason: sentence(Kind::of(content).phrase),
        disabled: None,
    }
}

/// What one side of a touched file is, as the viewer names it.
#[derive(Debug, Clone, Copy)]
struct Kind {
    /// In a word, as a Files row labels it.
    word: &'static str,
    /// As a sentence names it.
    phrase: &'static str,
}

impl Kind {
    /// What `content` is.
    fn of(content: &FileContent) -> Self {
        let (word, phrase) = match content {
            FileContent::Text(_) => ("text", "text"),
            FileContent::Binary { .. } => ("binary", "a binary file"),
            FileContent::Submodule => ("submodule", "a submodule"),
            FileContent::Symlink => ("symlink", "a symbolic link"),
        };
        Self { word, phrase }
    }
}

/// How many digits the last line number of `text` has.
fn digits(text: &str) -> usize {
    text.lines().count().to_string().len()
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

impl View for Viewer {
    fn route(&self) -> Route {
        Route::Viewer(self.at)
    }

    fn name(&self) -> &'static str {
        "Change"
    }

    fn keymap(&self) -> Keymap {
        match self.search_box {
            Some(_) => keymap::SEARCH_BOX,
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
        if self.search_box.is_some() {
            if let Some(query) = search::edit(&mut self.search_box, command) {
                self.search(query, page);
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
        // The summary, and right-aligned at least two cells after it the
        // areas a showcase change touches, or what a commit was compared
        // with. A commit's summary follows its short ID.
        let (summary, aside) = match self.change.origin() {
            Origin::Showcase { .. } => (
                self.change.summary().into_owned(),
                context.showcase.area_names(&self.change).join(", "),
            ),
            Origin::Commit { id, comparison } => (
                format!("{}  {}", id.short(), self.change.summary()),
                comparison_label(*comparison).unwrap_or_default().to_owned(),
            ),
        };
        let mut title = Line::from(Span::styled(summary, theme.title));
        let aside_column = usize::from(parts.title.width)
            .saturating_sub(Line::raw(&aside).width())
            .max(title.width() + columns::GAP);
        columns::pad_to(&mut title, aside_column);
        title.push_span(Span::styled(aside, theme.muted));
        frame.render_widget(Paragraph::new(title), parts.title);
        frame.render_stateful_widget(
            List::new(self.file_rows(theme, parts.files.width.saturating_sub(4))).block(
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
            Contents::Missing { reason, .. } => Paragraph::new(Line::styled(reason, theme.muted)),
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

/// The keys where a commit cannot be read: only back.
const UNREADABLE_KEYMAP: Keymap = &[keymap::BACK];

/// The actions where a commit cannot be read: only back.
const UNREADABLE_ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Return to the picker",
    command: Command::Back,
}];

/// What the change viewer shows instead of a commit it cannot read, such as
/// one whose files are missing from a partial clone: which commit, and why.
#[derive(Debug)]
pub(crate) struct Unreadable {
    /// Where it was opened: the viewer or the outbound review of the
    /// commit.
    route: Route,
    id: CommitId,
    /// Why it cannot be read, as gix says.
    problem: String,
}

impl Unreadable {
    /// The screen at `route` for the commit `id`, which cannot be read
    /// because of `error`.
    pub(crate) fn new(route: Route, id: CommitId, error: &LoadError) -> Self {
        use std::error::Error as _;

        Self {
            route,
            id,
            problem: error
                .source()
                .map_or_else(|| error.to_string(), ToString::to_string),
        }
    }
}

impl View for Unreadable {
    fn route(&self) -> Route {
        self.route
    }

    fn name(&self) -> &'static str {
        "Change"
    }

    fn keymap(&self) -> Keymap {
        UNREADABLE_KEYMAP
    }

    fn actions(&self) -> &'static [Action] {
        UNREADABLE_ACTIONS
    }

    fn apply(&mut self, _command: Command, _area: Rect) -> Option<Command> {
        None
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let [title, _, reason] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("Whetstone cannot read commit {}", self.id.short()),
                theme.title,
            )),
            title,
        );
        frame.render_widget(
            Paragraph::new(Line::styled(self.problem.as_str(), theme.muted))
                .wrap(Wrap { trim: true }),
            reason,
        );
    }
}
