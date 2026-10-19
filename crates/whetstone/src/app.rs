//! The app: all UI state, the events that change it, and how it is drawn.
//!
//! A key goes first to the reserved Ctrl+C, then to the current screen's
//! keymap, then to the keys that work everywhere. Whatever command it sends
//! is applied to the current screen, or to the app itself when it changes
//! screens or quits.

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Size};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::config::Settings;
use crate::help::KeyHelp;
use crate::home::{self, Home};
use crate::keymap::{self, Command, Keymap};
use crate::overlay::{Outcome, Overlay};
use crate::palette::{Action, Palette};
use crate::settings::{self, SettingsScreen};
use crate::theme::Theme;

/// The Whetstone TUI, independent of any real terminal.
#[derive(Debug)]
pub struct App {
    theme: Theme,
    settings: Settings,
    screens: Screens,
    /// The overlay over the current screen, if one is open.
    overlay: Option<Overlay>,
    /// The terminal's size, once the UI loop has reported it.
    viewport: Option<Size>,
}

/// Whether the UI loop keeps going after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running: draw the next frame and wait for the next event.
    Continue,
    /// Stop the loop and give the terminal back.
    Quit,
}

/// The smallest terminal the screens are drawn in. Below it the app shows a
/// notice instead, and keeps all its state for when the window grows again.
const MINIMUM: Size = Size::new(50, 12);

/// Whether a terminal of `size` is big enough for the screens.
fn fits(size: Size) -> bool {
    size.width >= MINIMUM.width && size.height >= MINIMUM.height
}

/// A screen a learner can go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Home,
    Settings,
}

/// A screen and its state.
#[derive(Debug, Clone)]
enum Screen {
    Home(Home),
    Settings(SettingsScreen),
}

impl Screen {
    /// The screen's name, as key help titles it.
    fn name(&self) -> &'static str {
        match self {
            Self::Home(_) => "Home",
            Self::Settings(_) => "Settings",
        }
    }

    /// The screen's own actions, which the palette lists before the ones
    /// every screen has.
    fn actions(&self) -> &'static [Action] {
        match self {
            Self::Home(_) | Self::Settings(_) => &[],
        }
    }

    /// The screen's own keys.
    fn keymap(&self) -> Keymap {
        match self {
            Self::Home(_) => home::KEYMAP,
            Self::Settings(_) => settings::KEYMAP,
        }
    }

    fn apply(&mut self, command: Command) -> Option<Command> {
        match self {
            Self::Home(home) => home.apply(command),
            Self::Settings(settings) => settings.apply(command),
        }
    }
}

/// The screens a learner went through to reach the current one, home first.
/// Going back returns to the one before, as it was left.
#[derive(Debug)]
struct Screens {
    home: Screen,
    above: Vec<Screen>,
}

impl Screens {
    fn current(&self) -> &Screen {
        self.above.last().unwrap_or(&self.home)
    }

    fn current_mut(&mut self) -> &mut Screen {
        match self.above.last_mut() {
            Some(screen) => screen,
            None => &mut self.home,
        }
    }

    /// Goes to `route`: home is always the one at the bottom, and any other
    /// screen opens above the current one unless it is the current one.
    fn go(&mut self, route: Route) {
        match (route, self.current()) {
            (Route::Home, _) => self.above.clear(),
            (Route::Settings, Screen::Settings(_)) => {}
            (Route::Settings, _) => self.above.push(Screen::Settings(SettingsScreen::default())),
        }
    }

    /// Returns to the screen before the current one, if there is one.
    fn back(&mut self) {
        self.above.pop();
    }
}

impl App {
    /// Starts on the home screen, drawing with `theme` and showing
    /// `settings` in Settings.
    #[must_use]
    pub fn new(theme: Theme, settings: Settings) -> Self {
        Self {
            theme,
            settings,
            screens: Screens {
                home: Screen::Home(Home::default()),
                above: Vec::new(),
            },
            overlay: None,
            viewport: None,
        }
    }

    /// Applies one terminal event. Key presses act, and a resize changes
    /// only how the next frame is laid out.
    pub fn handle(&mut self, event: &Event) -> Flow {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.press(key),
            Event::Resize(width, height) => {
                self.viewport = Some(Size::new(*width, *height));
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    /// Sends a key to the reserved Ctrl+C, then to the open overlay, which
    /// takes every key, or else to the current screen and then the keys that
    /// work everywhere.
    fn press(&mut self, key: &KeyEvent) -> Flow {
        if keymap::command(&[keymap::INTERRUPT], key).is_some() {
            return Flow::Quit;
        }
        // Under the notice nothing is visible, so no key changes anything.
        if self.viewport.is_some_and(|size| !fits(size)) {
            return Flow::Continue;
        }
        if let Some(overlay) = &mut self.overlay {
            let Some(command) = keymap::command(overlay.keymap(), key) else {
                return Flow::Continue;
            };
            return match overlay.apply(command, self.viewport) {
                Outcome::Stay => Flow::Continue,
                Outcome::Close => {
                    self.overlay = None;
                    Flow::Continue
                }
                Outcome::Run(command) => {
                    self.overlay = None;
                    self.run(command)
                }
            };
        }
        let command = keymap::command(self.screens.current().keymap(), key)
            .or_else(|| keymap::command(keymap::EVERYWHERE, key));
        match command {
            Some(command) => self.run(command),
            None => Flow::Continue,
        }
    }

    fn run(&mut self, command: Command) -> Flow {
        match command {
            Command::Quit => return Flow::Quit,
            Command::KeyHelp => {
                let screen = self.screens.current();
                self.overlay = Some(Overlay::KeyHelp(KeyHelp::new(
                    screen.name(),
                    screen.keymap(),
                )));
            }
            Command::Palette => {
                let screen = self.screens.current();
                self.overlay = Some(Overlay::Palette(Palette::new(
                    screen.name(),
                    screen.actions(),
                )));
            }
            Command::Go(route) => self.screens.go(route),
            Command::Back => self.screens.back(),
            command => {
                if let Some(next) = self.screens.current_mut().apply(command) {
                    return self.run(next);
                }
            }
        }
        Flow::Continue
    }

    /// Draws the current screen. Drawing never changes state.
    pub fn render(&self, frame: &mut Frame<'_>) {
        frame.render_widget(Block::new().style(self.theme.canvas), frame.area());
        if !fits(frame.area().as_size()) {
            self.render_too_small(frame);
            return;
        }
        let [body, footer] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
            .areas(frame.area().inner(Margin::new(2, 1)));
        let screen = self.screens.current();
        match screen {
            Screen::Home(home) => home.render(&self.theme, frame, body),
            Screen::Settings(screen) => screen.render(&self.theme, &self.settings, frame, body),
        }
        frame.render_widget(self.footer(screen.keymap()), footer);
        if let Some(overlay) = &self.overlay {
            overlay.render(&self.theme, frame);
        }
    }

    /// The notice shown instead of the screens in a window below
    /// [`MINIMUM`].
    fn render_too_small(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Window too small", self.theme.title),
                Line::raw(format!(
                    "Whetstone needs at least {}x{}; this window is {}x{}.",
                    MINIMUM.width, MINIMUM.height, area.width, area.height
                )),
                Line::styled(
                    "Resize it to carry on where you were, or press Ctrl+C to quit.",
                    self.theme.muted,
                ),
            ])
            .wrap(Wrap { trim: true }),
            area.inner(Margin::new(1, 0)),
        );
    }

    /// Reminders of the keys that matter most on a screen with `keymap`.
    fn footer(&self, keymap: Keymap) -> Paragraph<'static> {
        let hints: Vec<&str> = keymap::EVERYWHERE
            .iter()
            .chain(keymap)
            .filter_map(|binding| binding.hint)
            .collect();
        Paragraph::new(Line::styled(hints.join("  "), self.theme.muted))
    }
}
