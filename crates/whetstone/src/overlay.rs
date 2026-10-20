//! Overlays drawn over the current screen: key help, the palette of
//! actions, and asking before a draft would be lost. The open overlay owns
//! the keys, and the screen under it keeps its state, so closing the overlay
//! returns focus to the control that opened it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Margin, Rect, Size};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use crate::help::KeyHelp;
use crate::keymap::{self, Command, Keymap};
use crate::leave::Leave;
use crate::palette::Palette;
use crate::theme::Theme;

/// An overlay and its state.
#[derive(Debug, Clone)]
pub(crate) enum Overlay {
    KeyHelp(KeyHelp),
    Palette(Palette),
    Leave(Leave),
}

/// What an overlay asks of the app after a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Stay open.
    Stay,
    /// Close, returning focus to the screen under it.
    Close,
    /// Close, then run a command on the screen under it.
    Run(Command),
    /// Close, then run a command that loses a draft, as the learner chose
    /// to.
    Leave(Command),
}

impl Overlay {
    /// The overlay's keys. Every other key does nothing while it is open,
    /// except the reserved Ctrl+C and Ctrl+Z.
    pub(crate) fn keymap(&self) -> Keymap {
        match self {
            Self::KeyHelp(_) => KeyHelp::KEYMAP,
            Self::Palette(_) => Palette::KEYMAP,
            Self::Leave(_) => Leave::KEYMAP,
        }
    }

    /// Applies `command` in a terminal of `viewport` size.
    pub(crate) fn apply(&mut self, command: Command, viewport: Option<Size>) -> Outcome {
        match self {
            Self::KeyHelp(help) => help.apply(command, viewport),
            Self::Palette(palette) => palette.apply(command),
            Self::Leave(leave) => leave.apply(command),
        }
    }

    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>) {
        match self {
            Self::KeyHelp(help) => help.render(theme, frame),
            Self::Palette(palette) => palette.render(theme, frame),
            Self::Leave(leave) => leave.render(theme, frame),
        }
    }
}

/// A box for `rows` rows of `width` cells, centred in `area` and no bigger
/// than it allows. Returns the box and the body inside its border and
/// padding.
pub(crate) fn boxed(area: Rect, width: usize, rows: usize) -> (Rect, Rect) {
    // A border and a cell of padding on each side, a border above and below.
    let width = u16::try_from(width + 4).unwrap_or(u16::MAX);
    let height = u16::try_from(rows + 2).unwrap_or(u16::MAX);
    let outer = area.centered(
        Constraint::Length(width.min(area.width.saturating_sub(4))),
        Constraint::Length(height.min(area.height.saturating_sub(2))),
    );
    (outer, outer.inner(Margin::new(2, 1)))
}

/// Clears `area` and draws a box there titled `title`, with the footer hints
/// of `keymap` along its bottom edge.
pub(crate) fn draw_box(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    keymap: Keymap,
) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::bordered()
            .border_style(theme.muted)
            .title(Span::styled(format!(" {title} "), theme.title))
            .title_bottom(Line::styled(
                format!(" {} ", keymap::hints(&[keymap])),
                theme.muted,
            ))
            .style(theme.canvas),
        area,
    );
}
