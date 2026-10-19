//! The app: all UI state, the events that change it, and how it is drawn.

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Margin;
use ratatui::widgets::Block;

use crate::home;
use crate::theme::Theme;

/// The Whetstone TUI, independent of any real terminal.
#[derive(Debug)]
pub struct App {
    theme: Theme,
    screen: Screen,
}

/// Whether the UI loop keeps going after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running: draw the next frame and wait for the next event.
    Continue,
    /// Stop the loop and give the terminal back.
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
}

impl App {
    /// Starts on the home screen, drawing with `theme`.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            screen: Screen::Home,
        }
    }

    /// Applies one terminal event. Only key presses act.
    pub fn handle(&mut self, event: &Event) -> Flow {
        let Event::Key(key) = event else {
            return Flow::Continue;
        };
        if key.kind != KeyEventKind::Press {
            return Flow::Continue;
        }
        // In raw mode Ctrl+C arrives as a key; it quits from anywhere.
        if is_ctrl_c(key) {
            return Flow::Quit;
        }
        match self.screen {
            Screen::Home => home::handle(key),
        }
    }

    /// Draws the current screen. Drawing never changes state.
    pub fn render(&self, frame: &mut Frame<'_>) {
        frame.render_widget(Block::new().style(self.theme.canvas), frame.area());
        let area = frame.area().inner(Margin::new(2, 1));
        match self.screen {
            Screen::Home => home::render(&self.theme, frame, area),
        }
    }
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}
