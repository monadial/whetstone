//! A screen for reading text whole: what the outbound review sends for the
//! change, a file or an area, under a title, in sections that scroll
//! together. Code is numbered, so a line the review names can be found.

use std::fmt;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

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
    does: "Return to the outbound review",
    command: Command::Back,
}];

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Numbering {
    /// Prose, as written.
    Plain,
    /// Code, each line after its number, the first numbered `first`.
    Numbered { first: usize },
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

    /// The section's lines: its heading, then its text, each line after
    /// its number if it is code.
    fn lines(&self, theme: &Theme) -> Vec<Line<'_>> {
        let mut lines = vec![Line::styled(self.heading.as_str(), theme.muted)];
        let first = match self.numbering {
            Numbering::Plain => 1,
            Numbering::Numbered { first } => first,
        };
        let last = first + self.text.lines().count().max(1) - 1;
        let digits = last.to_string().len();
        for (index, line) in self.text.lines().enumerate() {
            lines.push(match self.numbering {
                Numbering::Plain => Line::raw(line),
                Numbering::Numbered { first } => Line::from(vec![
                    Span::styled(format!("{:>digits$} ", first + index), theme.muted),
                    Span::raw(line),
                ]),
            });
        }
        lines
    }
}

/// The reader's state: what it shows, and how far it is scrolled.
#[derive(Debug, Clone)]
pub(crate) struct Reader {
    route: Route,
    title: String,
    sections: Vec<Section>,
    /// The row at the top of the text.
    top: Position,
}

impl Reader {
    /// The reader at `route`, showing `sections` under `title`, from the
    /// top.
    pub(crate) fn new(route: Route, title: String, sections: Vec<Section>) -> Self {
        Self {
            route,
            title,
            sections,
            top: Position::default(),
        }
    }

    /// The text under the title, its sections a blank line apart.
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
        // Styles do not change how the text wraps.
        let last = self.last_top(&Theme::new(ColorSupport::NoColor), text);
        self.top = self.top.scrolled(command, last, usize::from(text.height));
        None
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let [title, _, text] = layout(area);
        frame.render_widget(
            Paragraph::new(Line::styled(self.title.as_str(), theme.title)),
            title,
        );
        let top = self.top.index().min(self.last_top(theme, text));
        frame.render_widget(
            Self::paragraph(self.text(theme)).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
            text,
        );
    }
}
