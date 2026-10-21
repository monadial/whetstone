//! A screen for reading text whole: what the outbound review sends for the
//! change, a file or an area, or the evidence Practice's questions rest on,
//! under a title, in sections that scroll together. Code is numbered, so a
//! line the review or a question names can be found. The questions screen
//! shows the code a question cites in the same sections (see
//! [`crate::cited_code`]), each line the change added or removed
//! marked as a diff marks it.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use whetstone_core::{DiffMark, Excerpt, ExcerptLine};

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap};
use crate::palette::Action;
use crate::theme::{ColorSupport, Theme};
use crate::view::{Context, Route, View};

/// The reader's own keys.
const KEYMAP: Keymap = &[
    Binding {
        does: "Scroll down or up",
        ..keymap::MOVE
    },
    keymap::ENDS,
    keymap::HALF_PAGE,
    keymap::BACK,
];

/// The reader's own action in the palette.
const ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Return to the screen it was opened from",
    command: Command::Back,
}];

/// How many lines above a line the reader opens at show too, so the code
/// leading up to it shows.
const LEAD: usize = 3;

/// One part of what the reader shows: a heading, and the text under it.
#[derive(Clone)]
pub(crate) struct Section {
    heading: String,
    text: String,
    numbering: Numbering,
}

/// Shows the text's length, never the text, which may hold a secret.
impl fmt::Debug for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Section")
            .field("heading", &self.heading)
            .field("text_bytes", &self.text.len())
            .field("numbering", &self.numbering)
            .finish()
    }
}

/// Whether a section's lines are numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Numbering {
    /// Prose, as written.
    Plain,
    /// Code, each line after its number, the first numbered `first`.
    Numbered { first: usize },
    /// An excerpt of code a question cites, in place of the text: each
    /// line after its number in its file, or none for a line the change
    /// removed, and its mark, `+` or `-`, where the change added or removed
    /// it. A line too wide for a row goes on under its text, not under its
    /// number.
    Excerpt(Vec<ExcerptLine>),
}

impl Section {
    /// Prose under `heading`.
    pub(crate) fn plain(heading: &str, text: &str) -> Self {
        Self {
            heading: heading.to_owned(),
            text: text.to_owned(),
            numbering: Numbering::Plain,
        }
    }

    /// Code under `heading`, each line numbered.
    pub(crate) fn numbered(heading: &str, text: &str) -> Self {
        Self::numbered_from(heading, text, 1)
    }

    /// Lines of code from a file under `heading`, each numbered as in the
    /// file, where the first is line `first`.
    pub(crate) fn numbered_from(heading: &str, text: &str, first: usize) -> Self {
        Self {
            numbering: Numbering::Numbered { first },
            ..Self::plain(heading, text)
        }
    }

    /// The lines of `excerpt` under `heading`, each numbered as in its
    /// file, and marked `+` or `-` where the change added or removed it, as
    /// its diff marks it.
    pub(crate) fn excerpt(heading: &str, excerpt: &Excerpt) -> Self {
        Self {
            heading: heading.to_owned(),
            text: String::new(),
            numbering: Numbering::Excerpt(excerpt.lines().to_vec()),
        }
    }

    /// The section's lines, `width` cells wide: its heading, then its
    /// text, each line after its number if it is code, or its excerpt.
    fn lines(&self, theme: &Theme, width: u16) -> Vec<Line<'_>> {
        let mut lines = vec![Line::styled(self.heading.as_str(), theme.muted)];
        match &self.numbering {
            Numbering::Plain => lines.extend(self.text.lines().map(Line::raw)),
            Numbering::Numbered { first } => {
                let last = first + self.text.lines().count().max(1) - 1;
                let digits = last.to_string().len();
                lines.extend(self.text.lines().enumerate().map(|(index, line)| {
                    Line::from(vec![
                        Span::styled(format!("{:>digits$} ", first + index), theme.muted),
                        Span::raw(line),
                    ])
                }));
            }
            Numbering::Excerpt(excerpt) => lines.extend(excerpt_rows(excerpt, theme, width)),
        }
        lines
    }
}

/// The rows of `excerpt`, drawn with `theme` `width` cells wide: each line
/// after its number and its mark, and the rest of a line too wide for its
/// row on the rows below it, under its text.
fn excerpt_rows<'a>(excerpt: &'a [ExcerptLine], theme: &Theme, width: u16) -> Vec<Line<'a>> {
    let digits = excerpt
        .iter()
        .filter_map(ExcerptLine::number)
        .max()
        .unwrap_or(1)
        .to_string()
        .len();
    // The number, a space, the mark and a space.
    let gutter = digits + 3;
    let room = usize::from(width).saturating_sub(gutter);
    let mut rows = Vec::new();
    for line in excerpt {
        let (sign, style) = match line.mark() {
            DiffMark::Unchanged => (' ', Style::new()),
            DiffMark::Added => ('+', theme.added),
            DiffMark::Removed => ('-', theme.removed),
        };
        let number = line
            .number()
            .map(|number| number.to_string())
            .unwrap_or_default();
        for (index, piece) in columns::cut(line.text(), room).into_iter().enumerate() {
            let mut spans = if index == 0 {
                vec![
                    Span::styled(format!("{number:>digits$} "), theme.muted),
                    Span::styled(format!("{sign} "), style),
                ]
            } else {
                vec![Span::raw(" ".repeat(gutter))]
            };
            spans.push(Span::styled(piece, style));
            rows.push(Line::from(spans));
        }
    }
    rows
}

/// Sections that scroll together: the reader's text under its title, and
/// the code a question cites beside it.
#[derive(Debug, Clone)]
pub(crate) struct Sections {
    sections: Vec<Section>,
    top: Top,
}

/// Where sections are scrolled to.
#[derive(Debug, Clone, Copy)]
enum Top {
    /// This row of the text, as it wraps, is at the top.
    Row(Position),
    /// A few lines above `line` of the section at `section`, before they
    /// wrap, but not above its heading, are at the top, until it is first
    /// scrolled, at the width it then shows at.
    Line { section: usize, line: usize },
}

impl Sections {
    /// `sections`, from the top.
    pub(crate) fn new(sections: Vec<Section>) -> Self {
        Self {
            sections,
            top: Top::Row(Position::default()),
        }
    }

    /// The same sections, from `line` of the section at `section`, counting
    /// from 0, with a few lines above it, but not above its heading.
    #[must_use]
    pub(crate) fn opening_at(self, section: usize, line: usize) -> Self {
        Self {
            top: Top::Line { section, line },
            ..self
        }
    }

    /// The text `width` cells wide, its sections a blank line apart, and
    /// where each section's heading is in it.
    fn text(&self, theme: &Theme, width: u16) -> (Text<'_>, Vec<usize>) {
        let mut lines = Vec::new();
        let mut headings = Vec::new();
        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                lines.push(Line::default());
            }
            headings.push(lines.len());
            lines.extend(section.lines(theme, width));
        }
        (Text::from(lines), headings)
    }

    /// The text's paragraph, wrapping long lines.
    fn paragraph(text: Text<'_>) -> Paragraph<'_> {
        Paragraph::new(text).wrap(Wrap { trim: false })
    }

    /// The furthest the text scrolls in `area`: to where its last row is at
    /// the bottom.
    fn last_top(&self, theme: &Theme, area: Rect) -> usize {
        self.rows_with(theme, area.width)
            .saturating_sub(usize::from(area.height))
    }

    /// How many rows the text takes `width` cells wide.
    pub(crate) fn rows(&self, width: u16) -> usize {
        // Styles do not change how the text wraps.
        self.rows_with(&Theme::new(ColorSupport::NoColor), width)
    }

    /// How many rows the text takes `width` cells wide, drawn with
    /// `theme`.
    fn rows_with(&self, theme: &Theme, width: u16) -> usize {
        Self::paragraph(self.text(theme, width).0).line_count(width)
    }

    /// The row at the top in `area`, measured with `theme`.
    fn top(&self, theme: &Theme, area: Rect) -> usize {
        let row = match self.top {
            Top::Row(row) => row.index(),
            Top::Line { section, line } => {
                let (mut text, headings) = self.text(theme, area.width);
                let heading = headings.get(section).copied().unwrap_or_default();
                text.lines
                    .truncate((heading + 1 + line).saturating_sub(LEAD).max(heading));
                Self::paragraph(text).line_count(area.width)
            }
        };
        row.min(self.last_top(theme, area))
    }

    /// Scrolls the text shown in `area` as `command` asks.
    pub(crate) fn scroll(&mut self, command: Command, area: Rect) {
        // Styles do not change how the text wraps.
        let theme = Theme::new(ColorSupport::NoColor);
        let last = self.last_top(&theme, area);
        let top = match self.top {
            Top::Row(row) => row,
            Top::Line { .. } => Position::at(self.top(&theme, area)),
        };
        let scrolled = top.scrolled(command, last, usize::from(area.height));
        // A command that doesn't scroll leaves the top as it was, even past
        // the end, so a window resized back shows what it showed before.
        if scrolled != top {
            self.top = Top::Row(scrolled);
        }
    }

    /// Draws the text in `area` with `theme`, from the row at the top.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let top = self.top(theme, area);
        frame.render_widget(
            Self::paragraph(self.text(theme, area.width).0)
                .scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
            area,
        );
    }
}

/// The reader's state: what it shows, and how far it is scrolled.
#[derive(Debug, Clone)]
pub(crate) struct Reader {
    route: Route,
    title: String,
    text: Sections,
}

impl Reader {
    /// The reader at `route`, showing `sections` under `title`, from the
    /// top.
    pub(crate) fn new(route: Route, title: String, sections: Vec<Section>) -> Self {
        Self {
            route,
            title,
            text: Sections::new(sections),
        }
    }

    /// The same reader, opening at `line` of the section at `section`,
    /// counting from 0, with a few lines above it.
    #[must_use]
    pub(crate) fn opening_at(self, section: usize, line: usize) -> Self {
        Self {
            text: self.text.opening_at(section, line),
            ..self
        }
    }
}

/// Where the reader's title and text go in `area`.
fn layout(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area)
}

impl View for Reader {
    fn route(&self) -> Route {
        self.route
    }

    fn name(&self) -> &'static str {
        "Reader"
    }

    fn keymap(&self) -> Keymap {
        KEYMAP
    }

    fn actions(&self) -> &'static [Action] {
        ACTIONS
    }

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        let [_, _, text] = layout(area);
        self.text.scroll(command, text);
        None
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let [title, _, text] = layout(area);
        frame.render_widget(
            Paragraph::new(Line::styled(self.title.as_str(), theme.title)),
            title,
        );
        self.text.render(theme, frame, text);
    }
}
