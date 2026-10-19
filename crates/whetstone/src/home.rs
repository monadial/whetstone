//! The home screen: the actions a learner starts from.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::app::Flow;
use crate::theme::Theme;

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

/// `q` quits from home.
pub(crate) fn handle(key: &KeyEvent) -> Flow {
    if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
        Flow::Quit
    } else {
        Flow::Continue
    }
}

pub(crate) fn render(theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
    let [header, actions, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(
        Paragraph::new(Line::styled("Whetstone", theme.title)),
        header,
    );
    frame.render_widget(
        List::new(
            Action::ALL
                .iter()
                .map(|action| ListItem::new(Line::styled(action.label(), theme.text))),
        ),
        actions,
    );
    frame.render_widget(Paragraph::new(Line::styled("q quit", theme.muted)), footer);
}
