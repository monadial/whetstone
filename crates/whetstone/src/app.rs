//! The app: all UI state, the events that change it, and how it is drawn.
//!
//! A key goes first to the reserved Ctrl+C, which quits from anywhere. Under
//! the "window too small" notice no other key acts. Otherwise the open
//! overlay takes every key. Without one, while a text entry on the screen
//! has focus, the key types into it, or the screen's keymap edits or leaves
//! it. Otherwise the screen's keymap and then the keys that work everywhere
//! turn it into a command. The command changes the current screen, or the
//! app itself when it opens an overlay, changes screens or quits.

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect, Size};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};
use whetstone_core::ShowcaseProject;

use crate::config::Settings;
use crate::help::KeyHelp;
use crate::home::Home;
use crate::keymap::{self, Command, Input};
use crate::overlay::{Outcome, Overlay};
use crate::palette::Palette;
use crate::repository::{Repository, Unavailable};
use crate::theme::Theme;
use crate::view::{Context, Route, View};

/// The Whetstone TUI, independent of any real terminal.
#[derive(Debug)]
pub struct App {
    theme: Theme,
    settings: Settings,
    /// The project Practice runs on.
    showcase: ShowcaseProject,
    /// The learner's repository as opened at startup, or why Practice on it
    /// is unavailable.
    repository: Result<Repository, Unavailable>,
    screens: Screens,
    /// The overlay over the current screen, if one is open.
    overlay: Option<Overlay>,
    /// The first key of a two-key sequence, waiting for the second.
    pending: keymap::Pending,
    /// The terminal's size as the UI loop last reported it. Drawing never
    /// uses it (it measures the frame). Key handling uses it to know whether
    /// the notice is showing, how far key help can scroll, and how far a
    /// page goes on the current screen.
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

/// Where the current screen and the footer under it go in a terminal of
/// `area`.
fn screen_and_footer(area: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)])
        .areas(area.inner(Margin::new(2, 1)))
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
    /// screen opens above the current one, unless it is the current one. The
    /// current screen builds it if it can, and otherwise it opens over the
    /// showcase project or the repository in `context`.
    fn go(&mut self, route: Route, context: &Context<'_>) {
        if route == Route::Home {
            self.above.clear();
        } else if route != self.current().route()
            && let Some(screen) = self
                .current()
                .open(route, context)
                .or_else(|| route.open(context.showcase, context.repository))
        {
            self.above.push(screen);
        }
    }

    /// Returns to the screen before the current one, if there is one.
    fn back(&mut self) {
        self.above.pop();
    }
}

impl App {
    /// Starts on the home screen, drawing with `theme`, showing `settings` in
    /// Settings, and practicing on `showcase` or on `repository`, the
    /// learner's repository as opened at startup, unless it says why Practice
    /// on it is unavailable.
    #[must_use]
    pub fn new(
        theme: Theme,
        settings: Settings,
        showcase: ShowcaseProject,
        repository: Result<Repository, Unavailable>,
    ) -> Self {
        let home = Home::new(repository.as_ref());
        Self {
            theme,
            settings,
            showcase,
            screens: Screens {
                home,
                above: Vec::new(),
            },
            repository,
            overlay: None,
            pending: keymap::Pending::default(),
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
        let screen = self.screens.current();
        let command = match screen.input() {
            // Rule 29: while a text entry has focus, every key but Ctrl+C
            // types, before any binding.
            Input::Text => keymap::typed(key)
                .map(Command::Type)
                .or_else(|| keymap::command(screen.keymap(), key)),
            Input::Commands => self
                .pending
                .command(&[screen.keymap(), keymap::EVERYWHERE], key),
        };
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
            Command::Go(route) => {
                let context = Context {
                    theme: &self.theme,
                    settings: &self.settings,
                    showcase: &self.showcase,
                    repository: self.repository.as_ref(),
                };
                self.screens.go(route, &context);
            }
            Command::Back => self.screens.back(),
            command => {
                // Until the loop reports a size, assume the smallest the
                // screens are drawn in.
                let size = self.viewport.unwrap_or(MINIMUM);
                let [screen, _] = screen_and_footer(Rect::from((Position::ORIGIN, size)));
                if let Some(next) = self.screens.current_mut().apply(command, screen) {
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
        let [body, footer] = screen_and_footer(frame.area());
        let screen = self.screens.current();
        let context = Context {
            theme: &self.theme,
            settings: &self.settings,
            showcase: &self.showcase,
            repository: self.repository.as_ref(),
        };
        screen.render(&context, frame, body);
        // While a text entry has focus, the keys that work everywhere type.
        let hints = match screen.input() {
            Input::Text => keymap::hints(&[screen.keymap()]),
            Input::Commands => keymap::hints(&[keymap::EVERYWHERE, screen.keymap()]),
        };
        frame.render_widget(
            Paragraph::new(Line::styled(hints, self.theme.muted)),
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
