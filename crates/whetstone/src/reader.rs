//! A screen for reading text whole: what the outbound review sends for the
//! change, a file or an area, or the evidence Practice's questions rest on,
//! under a title, in sections that scroll together. Code is numbered, so a
//! line the review or a question names can be found. The questions screen
//! shows the code a question cites in the same sections (see
//! [`crate::question_context`]), each line the change added or removed
//! marked as a diff marks it.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use whetstone_core::{DiffMark, ExcerptLine};

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
    /// Code from a touched file, each line after its number, or none for a
    /// line the change removed, then how the change marks it: one for each
    /// line of the text.
    Marked(Vec<(Option<usize>, DiffMark)>),
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

    /// Lines of code from a touched file under `heading`, each numbered as
    /// in the file, and marked `+` or `-` where the change added or removed
    /// it, as its diff marks it.
    pub(crate) fn marked(heading: &str, lines: &[ExcerptLine]) -> Self {
        Self {
            heading: heading.to_owned(),
            // Each line ends in a line break, so an empty last line stays.
            text: lines.iter().flat_map(|line| [line.text(), "\n"]).collect(),
            numbering: Numbering::Marked(
                lines
                    .iter()
                    .map(|line| (line.number(), line.mark()))
                    .collect(),
            ),
        }
    }

    /// How many lines it takes before it wraps: its heading and its text.
    fn len(&self) -> usize {
        1 + self.text.lines().count()
    }

    /// The section's lines: its heading, then its text, each line after
    /// its number if it is code, and its mark if the change added or
    /// removed it.
    fn lines(&self, theme: &Theme) -> Vec<Line<'_>> {
        let mut lines = vec![Line::styled(self.heading.as_str(), theme.muted)];
        let digits = match &self.numbering {
            Numbering::Plain => 1,
            Numbering::Numbered { first } => (first + self.text.lines().count().max(1) - 1)
                .to_string()
                .len(),
            Numbering::Marked(marks) => marks
                .iter()
                .filter_map(|&(number, _)| number)
                .max()
                .unwrap_or(1)
                .to_string()
                .len(),
        };
        for (index, line) in self.text.lines().enumerate() {
            lines.push(match &self.numbering {
                Numbering::Plain => Line::raw(line),
                Numbering::Numbered { first } => Line::from(vec![
                    Span::styled(format!("{:>digits$} ", first + index), theme.muted),
                    Span::raw(line),
                ]),
                Numbering::Marked(marks) => {
                    let (number, mark) = marks
                        .get(index)
                        .copied()
                        .unwrap_or((None, DiffMark::Unchanged));
                    let (sign, style) = match mark {
                        DiffMark::Unchanged => (' ', Style::new()),
                        DiffMark::Added => ('+', theme.added),
                        DiffMark::Removed => ('-', theme.removed),
                    };
                    let number = number.map(|number| number.to_string()).unwrap_or_default();
                    Line::from(vec![
                        Span::styled(format!("{number:>digits$} "), theme.muted),
                        Span::styled(format!("{sign} "), style),
                        Span::styled(line, style),
                    ])
                }
            });
        }
        lines
    }
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
    /// This line of the text, before it wraps, is at the top, until it is
    /// first scrolled, at the width it then shows at.
    Line(usize),
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
        let heading: usize = self
            .sections
            .iter()
            .take(section)
            .map(|section| section.len() + 1)
            .sum();
        Self {
            top: Top::Line((heading + 1 + line).saturating_sub(LEAD).max(heading)),
            ..self
        }
    }

    /// The text, its sections a blank line apart.
    fn text(&self, theme: &Theme) -> Text<'_> {
        let mut lines = Vec::new();
        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                lines.push(Line::default());
            }
            lines.extend(section.lines(theme));
        }
        Text::from(lines)
    }

    /// The text's paragraph, wrapping long lines.
    fn paragraph(text: Text<'_>) -> Paragraph<'_> {
        Paragraph::new(text).wrap(Wrap { trim: false })
    }

    /// The furthest the text scrolls in `area`: to where its last row is at
    /// the bottom.
    fn last_top(&self, theme: &Theme, area: Rect) -> usize {
        Self::paragraph(self.text(theme))
            .line_count(area.width)
            .saturating_sub(usize::from(area.height))
    }

    /// How many rows the text takes `width` cells wide.
    pub(crate) fn rows(&self, width: u16) -> usize {
        // Styles do not change how the text wraps.
        Self::paragraph(self.text(&Theme::new(ColorSupport::NoColor))).line_count(width)
    }

    /// The row at the top in `area`, measured with `theme`.
    fn top(&self, theme: &Theme, area: Rect) -> usize {
        let row = match self.top {
            Top::Row(row) => row.index(),
            Top::Line(line) => {
                let mut text = self.text(theme);
                text.lines.truncate(line);
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
            Top::Line(_) => Position::at(self.top(&theme, area)),
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
            Self::paragraph(self.text(theme)).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
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
