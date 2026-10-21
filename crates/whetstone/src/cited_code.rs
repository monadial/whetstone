//! The code a question is about, beside it on the questions screen and on
//! its feedback: a header naming the change, then each citation's lines as
//! the evidence sends them, under its path and line range, with a few lines
//! around them. Lines are numbered as in their file. In a touched file, the
//! lines the change added or removed are marked `+` and `-`, as its diff
//! marks them; in connected code, a related test or related configuration,
//! the heading says why it is there, such as "calls `publish`".
//!
//! The pane reads only the evidence Practice holds, exactly as it was sent,
//! never the repository. It scrolls on its own while it has focus, and `z`
//! shows it on the whole screen, and back.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use whetstone_core::{Evidence, ExcerptSource, Extent, Question};

use crate::keymap::{self, Binding, Command, Press};
use crate::reader::{Section, Sections};
use crate::theme::{ControlState, Theme};

/// How many lines the pane shows on either side of the cited lines, as far
/// as the evidence sends them.
const AROUND: usize = 3;

/// `j`/`k` and the arrows scroll the code while it has focus.
pub(crate) const SCROLL: Binding = Binding {
    does: "Scroll the code down or up",
    ..keymap::MOVE
};

/// Page Down and Page Up scroll the code a page.
pub(crate) const PAGE: Binding = Binding {
    keys: &[
        (Press::key(KeyCode::PageDown), Command::PageDown),
        (Press::key(KeyCode::PageUp), Command::PageUp),
    ],
    does: "Scroll the code a page down or up",
    hint: Some("PgDn/Up page"),
};

/// `gg` and `G` scroll to either end of the code.
pub(crate) const ENDS: Binding = Binding {
    does: "Scroll to the top or bottom of the code",
    ..keymap::ENDS
};

/// Ctrl+D and Ctrl+U scroll the code half a page.
pub(crate) const HALF_PAGE: Binding = Binding {
    does: "Scroll the code down or up half a page",
    ..keymap::HALF_PAGE
};

/// `z` shows the code on the whole screen, from the question or its
/// feedback.
pub(crate) const ZOOM: Binding = Binding {
    keys: &[(Press::char('z'), Command::Zoom)],
    does: "Show the code the question cites on the whole screen",
    hint: Some("z code"),
};

/// `z`, `q` and Esc go back to the question from the code on the whole
/// screen.
pub(crate) const UNZOOM: Binding = Binding {
    keys: &[
        (Press::char('z'), Command::Zoom),
        (Press::char('q'), Command::Zoom),
        (Press::key(KeyCode::Esc), Command::Zoom),
    ],
    does: "Back to the question",
    hint: Some("z back"),
};

/// The keys while the code shows on the whole screen.
pub(crate) const ZOOMED_KEYMAP: keymap::Keymap = &[SCROLL, PAGE, ENDS, HALF_PAGE, UNZOOM];

/// The code one question cites, and how far it is scrolled.
#[derive(Debug, Clone)]
pub(crate) struct CitedCode {
    /// The change, as the header names it.
    header: String,
    code: Sections,
}

impl CitedCode {
    /// The code `question` cites, as `evidence` sends it, under `header`,
    /// which names the change.
    pub(crate) fn new(header: String, question: &Question, evidence: &Evidence) -> Self {
        let sections = question
            .citations()
            .iter()
            .filter_map(|citation| {
                // Plain checks drop a citation the evidence doesn't send.
                let excerpt = citation.excerpt(evidence, AROUND)?;
                let mut heading =
                    format!("{}, {}", citation.path(), Extent::Lines(citation.lines()));
                if let ExcerptSource::Connection { relations } = excerpt.source() {
                    // Relations to different touched files can read alike.
                    let mut said: Vec<String> = Vec::new();
                    for relation in relations.iter().map(ToString::to_string) {
                        if !said.contains(&relation) {
                            said.push(relation);
                        }
                    }
                    heading = format!("{heading} · {}", said.join(", "));
                }
                Some(Section::excerpt(&heading, &excerpt))
            })
            .collect();
        Self {
            header,
            code: Sections::new(sections),
        }
    }

    /// How many rows the pane takes `width` cells wide to show all of its
    /// code, its header included.
    pub(crate) fn rows(&self, width: u16) -> u16 {
        u16::try_from(self.code.rows(width))
            .unwrap_or(u16::MAX)
            .saturating_add(1)
    }

    /// Scrolls the code in the pane drawn in `area` as `command` asks.
    pub(crate) fn scroll(&mut self, command: Command, area: Rect) {
        let [_, code] = layout(area);
        self.code.scroll(command, code);
    }

    /// Draws the pane in `area` with `theme`: the header, marked while the
    /// pane has focus, as a control is, then the code.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [header, code] = layout(area);
        let line: Line<'_> = theme.control(
            self.header.as_str(),
            ControlState {
                focused,
                ..ControlState::default()
            },
        );
        let line = if focused {
            line
        } else {
            line.patch_style(theme.title)
        };
        frame.render_widget(Paragraph::new(line), header);
        self.code.render(theme, frame, code);
    }
}

/// Where the pane's header and code go in `area`.
fn layout(area: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area)
}
