//! The home screen: the flat-stone mark, the actions a learner starts from,
//! and what Practice will send once AI arrives.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph, Wrap};

use crate::app::Route;
use crate::focus::Position;
use crate::keymap::{self, Command, Keymap};
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

    /// Why the action cannot be used yet, if it cannot.
    fn unavailable(self) -> Option<&'static str> {
        match self {
            Self::Practice => Some("arrives with the change picker"),
            Self::Settings | Self::Quit => None,
        }
    }

    /// What activating it asks of the app.
    fn command(self) -> Option<Command> {
        match self {
            Self::Practice => None,
            Self::Settings => Some(Command::Go(Route::Settings)),
            Self::Quit => Some(Command::Quit),
        }
    }
}

/// The brand's primary claim, beside the name.
const CLAIM: &str = "Understand the code you ship.";
/// The cells between the mark and the name beside it.
const NAME_GAP: u16 = 4;
/// The cells the name and the claim beside the mark need.
const NAME_WIDTH: u16 = 29;

/// Where data goes, said before anything is ever sent.
const OUTBOUND_NOTICE: &str = "Once AI arrives, Practice will send showcase excerpts and your answers \
                        to OpenRouter, paid with your credits.";

/// Home's own keys: `q` quits from here.
pub(crate) const KEYMAP: Keymap = &[keymap::MOVE, keymap::ACTIVATE, keymap::QUIT];

/// The home screen's state: which action has focus.
#[derive(Debug, Clone, Default)]
pub(crate) struct Home {
    focus: Position,
}

impl Home {
    /// Applies `command`, and returns what it asks of the app, if anything.
    pub(crate) fn apply(&mut self, command: Command) -> Option<Command> {
        let count = Action::ALL.len();
        match command {
            Command::Down => self.focus = self.focus.down(count),
            Command::Up => self.focus = self.focus.up(),
            Command::NextControl => self.focus = self.focus.next(count),
            Command::PreviousControl => self.focus = self.focus.previous(count),
            Command::Activate => return self.focused().command(),
            _ => {}
        }
        None
    }

    fn focused(&self) -> Action {
        Action::ALL[self.focus.index()]
    }

    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let outbound =
            Paragraph::new(Line::styled(OUTBOUND_NOTICE, theme.muted)).wrap(Wrap { trim: true });
        let actions_rows = u16::try_from(Action::ALL.len()).unwrap_or(u16::MAX);
        let outbound_rows = u16::try_from(outbound.line_count(area.width)).unwrap_or(u16::MAX);
        // The mark needs room beside it for the name, and above the rest.
        let with_mark = area.width >= mark::WIDTH + NAME_GAP + NAME_WIDTH
            && area.height >= mark::HEIGHT + 1 + actions_rows + 1 + outbound_rows;
        let [brand, _, actions, _, outbound_area] = Layout::vertical([
            Constraint::Length(if with_mark { mark::HEIGHT } else { 1 }),
            Constraint::Length(1),
            Constraint::Length(actions_rows),
            Constraint::Length(1),
            // The line wraps in narrower terminals.
            Constraint::Fill(1),
        ])
        .areas(area);

        let name = Line::from(vec![
            Span::styled("whetstone", theme.title),
            Span::styled(" by Monadial", theme.muted),
        ]);
        if with_mark {
            let [stone, _, beside] = Layout::horizontal([
                Constraint::Length(mark::WIDTH),
                Constraint::Length(NAME_GAP),
                Constraint::Fill(1),
            ])
            .areas(brand);
            frame.render_widget(Paragraph::new(mark::text(theme.stone)), stone);
            frame.render_widget(
                Paragraph::new(vec![Line::default(), name, Line::raw(CLAIM)]),
                beside,
            );
        } else {
            // A short or narrow window keeps the name: the brand's text fallback.
            frame.render_widget(Paragraph::new(name), brand);
        }
        frame.render_widget(
            List::new(Action::ALL.map(|action| {
                theme.control(
                    action.label(),
                    ControlState {
                        focused: action == self.focused(),
                        disabled: action.unavailable(),
                        ..ControlState::default()
                    },
                )
            })),
            actions,
        );
        frame.render_widget(outbound, outbound_area);
    }
}
