//! Key help: the keys of the screen under it, read from the same keymaps
//! that dispatch keys, so it always says what the keys do.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Position, Rect, Size};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::overlay::{self, Outcome};
use crate::theme::Theme;

/// Key help for one screen: its own keys, then the ones that work
/// everywhere. It scrolls when the terminal is too short to show them all.
#[derive(Debug, Clone)]
pub(crate) struct KeyHelp {
    /// The screen's name, for the title.
    screen: &'static str,
    /// The screen's own keys.
    keymap: Keymap,
    /// How many rows are scrolled off the top.
    scroll: usize,
}

impl KeyHelp {
    pub(crate) const KEYMAP: Keymap = &[
        Binding {
            does: "Scroll down or up",
            hint: Some("j/k scroll"),
            ..keymap::MOVE
        },
        Binding {
            keys: &[
                (Press::key(KeyCode::Esc), Command::Back),
                (Press::char('q'), Command::Back),
                (Press::char('?'), Command::Back),
            ],
            does: "Close",
            hint: Some("Esc close"),
        },
    ];

    /// Key help for the screen called `screen`, whose own keys are
    /// `keymap`.
    pub(crate) fn new(screen: &'static str, keymap: Keymap) -> Self {
        Self {
            screen,
            keymap,
            scroll: 0,
        }
    }

    pub(crate) fn apply(&mut self, command: Command, viewport: Option<Size>) -> Outcome {
        match command {
            Command::Down => {
                let rows = self.rows();
                let last = viewport.map_or(rows.len().saturating_sub(1), |size| {
                    last_scroll(Rect::from((Position::ORIGIN, size)), &rows)
                });
                self.scroll = (self.scroll + 1).min(last);
            }
            Command::Up => self.scroll = self.scroll.saturating_sub(1),
            Command::Back => return Outcome::Close,
            _ => {}
        }
        Outcome::Stay
    }

    /// The rows of key help: the screen's own keys, then a heading, then the
    /// keys that work everywhere.
    fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = self.keymap.iter().map(Row::Keys).collect();
        rows.push(Row::Blank);
        rows.push(Row::Heading("Everywhere"));
        rows.extend(keymap::EVERYWHERE.iter().map(Row::Keys));
        rows
    }

    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>) {
        let rows = self.rows();
        let (outer, body) = layout(frame.area(), &rows);
        let keys_width = Row::keys_width(&rows);
        // A smaller terminal than the one it was scrolled in shows the end.
        let scroll = self.scroll.min(last_scroll(frame.area(), &rows));
        overlay::draw_box(
            theme,
            frame,
            outer,
            &format!("Keys on {}", self.screen),
            Self::KEYMAP,
        );
        frame.render_widget(
            Paragraph::new(
                rows.iter()
                    .skip(scroll)
                    .map(|row| row.line(theme, keys_width))
                    .collect::<Vec<_>>(),
            ),
            body,
        );
    }
}

/// Where the box and its body go in `area`.
fn layout(area: Rect, rows: &[Row]) -> (Rect, Rect) {
    let keys_width = Row::keys_width(rows);
    let width = rows
        .iter()
        .map(|row| row.width(keys_width))
        .max()
        .unwrap_or_default();
    overlay::boxed(area, width, rows.len())
}

/// The furthest `rows` can scroll in a terminal of `area`.
fn last_scroll(area: Rect, rows: &[Row]) -> usize {
    let (_, body) = layout(area, rows);
    rows.len().saturating_sub(usize::from(body.height))
}

/// A row of key help.
#[derive(Debug, Clone, Copy)]
enum Row {
    Keys(&'static Binding),
    Blank,
    Heading(&'static str),
}

impl Row {
    /// The widest list of keys in `rows`, plus the gap after it.
    fn keys_width(rows: &[Self]) -> usize {
        rows.iter()
            .filter_map(|row| match row {
                Self::Keys(binding) => Some(Line::raw(binding.key_names()).width()),
                Self::Blank | Self::Heading(_) => None,
            })
            .max()
            .unwrap_or_default()
            + 2
    }

    fn width(self, keys_width: usize) -> usize {
        match self {
            Self::Keys(binding) => keys_width + Line::raw(binding.does).width(),
            Self::Blank => 0,
            Self::Heading(heading) => Line::raw(heading).width(),
        }
    }

    fn line(self, theme: &Theme, keys_width: usize) -> Line<'static> {
        match self {
            Self::Keys(binding) => {
                let keys = binding.key_names();
                let gap = keys_width.saturating_sub(Line::raw(keys.as_str()).width());
                Line::from(vec![
                    Span::styled(keys, theme.title),
                    Span::raw(" ".repeat(gap)),
                    Span::raw(binding.does),
                ])
            }
            Self::Blank => Line::default(),
            Self::Heading(heading) => Line::styled(heading, theme.muted),
        }
    }
}
