//! The app: all UI state, the events that change it, and how it is drawn.
//!
//! A key goes first to the reserved Ctrl+C, which quits from anywhere. Under
//! the "window too small" notice no other key acts. Otherwise the open
//! overlay takes every key, or else the current screen's keymap and then the
//! keys that work everywhere turn it into a command. The command changes the
//! current screen, or the app itself when it opens an overlay, changes
//! screens or quits.

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Size};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};
use whetstone_core::ShowcaseProject;

use crate::config::Settings;
use crate::help::KeyHelp;
use crate::home::Home;
use crate::keymap::{self, Command};
use crate::overlay::{Outcome, Overlay};
use crate::palette::Palette;
use crate::theme::Theme;
use crate::view::{Context, Route, View};

/// The Whetstone TUI, independent of any real terminal.
#[derive(Debug)]
pub struct App {
    theme: Theme,
    settings: Settings,
    /// The project Practice runs on.
    showcase: ShowcaseProject,
    screens: Screens,
    /// The overlay over the current screen, if one is open.
    overlay: Option<Overlay>,
    /// The terminal's size as the UI loop last reported it. Layout never
    /// uses it (drawing measures the frame); it only tells key handling
    /// whether the notice is showing and how far key help can scroll.
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

/// The screens a learner went through to reach the current one, home first.
/// Going back returns to the one before, as it was left.
#[derive(Debug)]
struct Screens {
    home: Home,
    above: Vec<Box<dyn View>>,
}

impl Screens {
    fn current(&self) -> &dyn View {
        self.above.last().map_or(&self.home, AsRef::as_ref)
    }

    fn current_mut(&mut self) -> &mut dyn View {
        match self.above.last_mut() {
            Some(screen) => screen.as_mut(),
            None => &mut self.home,
        }
    }

    /// Goes to `route`: home is always the one at the bottom, and any other
    /// screen opens above the current one, over `showcase`, unless it is the
    /// current one.
    fn go(&mut self, route: Route, showcase: &ShowcaseProject) {
        if route == Route::Home {
            self.above.clear();
        } else if route != self.current().route() {
            self.above.push(route.open(showcase));
        }
    }

    /// Returns to the screen before the current one, if there is one.
    fn back(&mut self) {
        self.above.pop();
    }
}

impl App {
    /// Starts on the home screen, drawing with `theme`, showing `settings` in
    /// Settings and practicing on `showcase`.
    #[must_use]
    pub fn new(theme: Theme, settings: Settings, showcase: ShowcaseProject) -> Self {
        Self {
            theme,
            settings,
            showcase,
            screens: Screens {
                home: Home::default(),
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
                self.overlay = Some(Overlay::KeyHelp(KeyHelp::new(self.screens.current())));
            }
            Command::Palette => {
                self.overlay = Some(Overlay::Palette(Palette::new(self.screens.current())));
            }
            Command::Go(route) => self.screens.go(route, &self.showcase),
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
        let context = Context {
            theme: &self.theme,
            settings: &self.settings,
            showcase: &self.showcase,
        };
        screen.render(&context, frame, body);
        frame.render_widget(
            Paragraph::new(Line::styled(
                keymap::hints(&[keymap::EVERYWHERE, screen.keymap()]),
                self.theme.muted,
            )),
            footer,
        );
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
}
