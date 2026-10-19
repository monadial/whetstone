//! The home screen: the flat-stone mark, the actions a learner starts from,
//! and what Practice will send once AI arrives.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph, Wrap};

use crate::app::Flow;
use crate::mark;
use crate::theme::{ControlState, Theme};

/// What a learner can start from home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Practice,
    Settings,
    Quit,
}

impl Action {
    const ALL: [Self; 3] = [Self::Practice, Self::Settings, Self::Quit];

    fn label(self) -> &'static str {
        match self {
            Self::Practice => "Practice on the showcase project",
            Self::Settings => "Settings",
            Self::Quit => "Quit",
        }
    }
}

/// Where learner data goes, said before anything is ever sent.
const OUTBOUND_NOTICE: &str = "Once AI arrives, Practice will send showcase excerpts and your answers \
                        to OpenRouter, paid with your credits.";

/// `q` quits from home.
pub(crate) fn handle(key: &KeyEvent) -> Flow {
    if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
        Flow::Quit
    } else {
        Flow::Continue
    }
}

pub(crate) fn render(theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
    let [brand, _, actions, _, outbound, footer] = Layout::vertical([
        Constraint::Length(mark::HEIGHT),
        Constraint::Length(1),
        Constraint::Length(u16::try_from(Action::ALL.len()).unwrap_or(u16::MAX)),
        Constraint::Length(1),
        // The line wraps in narrower terminals.
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let [stone, _, name] = Layout::horizontal([
        Constraint::Length(mark::WIDTH),
        Constraint::Length(4),
        Constraint::Fill(1),
    ])
    .areas(brand);

    frame.render_widget(Paragraph::new(mark::text(theme.stone)), stone);
    frame.render_widget(
        Paragraph::new(vec![
            Line::default(),
            Line::from(vec![
                Span::styled("whetstone", theme.title),
                Span::styled(" by Monadial", theme.muted),
            ]),
            Line::raw("Understand the code you ship."),
        ]),
        name,
    );
    // Focus starts on Practice; the keymap moves it.
    frame.render_widget(
        List::new(Action::ALL.map(|action| {
            theme.control(
                action.label(),
                ControlState {
                    focused: action == Action::Practice,
                    ..ControlState::default()
                },
            )
        })),
        actions,
    );
    frame.render_widget(
        Paragraph::new(Line::styled(OUTBOUND_NOTICE, theme.muted)).wrap(Wrap { trim: true }),
        outbound,
    );
    frame.render_widget(Paragraph::new(Line::styled("q quit", theme.muted)), footer);
}
