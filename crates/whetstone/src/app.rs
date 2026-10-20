//! The app: all UI state, the events that change it, and how it is drawn.
//!
//! A key goes first to the reserved keys: Ctrl+C quits from anywhere, and
//! Ctrl+Z suspends from anywhere until `fg`, without asking. Under the "window too small"
//! notice no other key acts. Otherwise the open overlay takes every key.
//! Without one, while a text entry on the screen has focus, the key types
//! into it, or the screen's keymap edits or leaves it. Otherwise the
//! screen's keymap and then the keys that work everywhere turn it into a
//! command. The command changes the current screen, or the app itself when
//! it opens an overlay, changes screens, quits or suspends. Before going
//! back, home or quitting would lose a draft a screen holds, the app asks.
//!
//! A paste never becomes keys or commands. It goes into the screen's focused
//! text entry as one insertion, and anywhere else it does nothing.
//!
//! A background job's result enters the same way, and is applied only if
//! the app still waits for that job. So does the end of a job's time limit,
//! after which the app no longer waits for it. Once the screen has applied a
//! command, a result or a time limit, it saves what its state now holds for
//! the session record, then starts any job its state now needs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect, Size};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};
use whetstone_ai::Ai;
use whetstone_core::ShowcaseProject;

use crate::config::Settings;
use crate::connected_code::{self, AnalysisDirs, AnalysisDirsGuard, GraphCache};
use crate::failures::RefusedModels;
use crate::help::KeyHelp;
use crate::home::Home;
use crate::job::{Finished, Job, JobId, JobOutcome, JobOutput, Jobs, TimeLimit};
use crate::keymap::{self, Command, Input};
use crate::leave::Leave;
use crate::overlay::{Outcome, Overlay};
use crate::palette::Palette;
use crate::repository::{Repository, Unavailable};
use crate::session_record::{InMemoryStore, SessionStore};
use crate::theme::Theme;
use crate::view::{Context, Route, View};

/// The Whetstone TUI, independent of any real terminal.
#[derive(Debug)]
pub struct App {
    theme: Theme,
    settings: Settings,
    /// The showcase project.
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
    /// The background jobs the app started.
    jobs: Jobs<JobOutput>,
    /// How many times the UI loop has ticked, which moves a spinner while a
    /// job runs.
    ticks: usize,
    /// Where connected code is analysed.
    analysis: AnalysisDirs,
    /// Removes the analysis directories still in use when the app drops,
    /// as it quits, even while a job is still writing or analysing one.
    _analysis_guard: AnalysisDirsGuard,
    /// The graphs connected code was found in before, one cache for the
    /// whole run, shared with the jobs that find connected code.
    cache: Arc<GraphCache>,
    /// How long the outbound review, and Preparing questions on the
    /// showcase project, wait for connected code.
    time_limit: Duration,
    /// Jev and the generator, if Practice can ask them.
    ai: Option<Arc<dyn Ai>>,
    /// The models `OpenRouter` refused during this run for having no
    /// zero-retention endpoint, which Settings marks until Whetstone quits.
    refused_models: Arc<RefusedModels>,
    /// Where session records are kept: in memory until the durable store
    /// arrives.
    store: Box<dyn SessionStore>,
}

/// Something for the app to apply: a terminal event the UI loop read, a
/// background job's result, the end of a job's time limit, or a tick while
/// a job runs.
#[derive(Debug)]
pub enum Message {
    /// Only key presses and pastes act (rule 26). A resize changes only how
    /// the next frame is laid out.
    Terminal(Event),
    /// A background job ended.
    Finished(Finished<JobOutput>),
    /// The time limit the app set on this job ran out. Unless the job has
    /// ended, the app gives up waiting for it, without cancelling it.
    TimeUp(JobId),
    /// While a job runs, the UI loop ticks to draw the next frame.
    Tick,
}

/// Whether the UI loop keeps going after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running: draw the next frame and wait for the next event.
    Continue,
    /// Stop the loop and give the terminal back.
    Quit,
    /// Give the terminal back and stop the process until it is continued,
    /// then take the terminal back and redraw the same screen.
    Suspend,
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
    /// showcase project or the repository in `context`. A screen that opens
    /// starts its background jobs with `jobs`, and going home closes every
    /// screen above it, the current one first.
    fn go(&mut self, route: Route, context: &Context<'_>, jobs: &mut Jobs<JobOutput>) {
        if route == Route::Home {
            while !self.above.is_empty() {
                self.back(jobs);
            }
        } else if route != self.current().route()
            && let Some(screen) = self.open(route, context)
        {
            self.push(screen, jobs);
        }
    }

    /// Goes to `route` in place of the current screen, which builds it if
    /// it can, as [`Screens::go`] does. The current screen then closes with
    /// `jobs`, and the new one opens above the screen before it and starts
    /// its background jobs, so going back from it returns there.
    fn replace(&mut self, route: Route, context: &Context<'_>, jobs: &mut Jobs<JobOutput>) {
        if let Some(screen) = self.open(route, context) {
            self.back(jobs);
            self.push(screen, jobs);
        }
    }

    /// The screen at `route`, built by the current screen if it can, and
    /// otherwise opened over the showcase project or the repository in
    /// `context`.
    fn open(&self, route: Route, context: &Context<'_>) -> Option<Box<dyn View>> {
        self.current()
            .open(route, context)
            .or_else(|| route.open(context))
    }

    /// Opens `screen` above the current one, starting its background jobs
    /// with `jobs`.
    fn push(&mut self, mut screen: Box<dyn View>, jobs: &mut Jobs<JobOutput>) {
        screen.start_jobs(jobs);
        self.above.push(screen);
    }

    /// The open screen that waits for the job `job`, if one does.
    fn waiting_for(&mut self, job: JobId) -> Option<&mut dyn View> {
        if self.home.waits_for(job) {
            return Some(&mut self.home);
        }
        self.above
            .iter_mut()
            .find(|screen| screen.waits_for(job))
            .map(|screen| screen.as_mut() as &mut dyn View)
    }

    /// Returns to the screen before the current one, if there is one,
    /// closing the current one with `jobs`.
    fn back(&mut self, jobs: &mut Jobs<JobOutput>) {
        if let Some(mut screen) = self.above.pop() {
            screen.close(jobs);
        }
    }
}

impl App {
    /// Starts on the home screen, drawing with `theme`, showing `settings` in
    /// Settings, and practicing on `showcase` or on `repository`, the
    /// learner's repository as opened at startup, unless it says why Practice
    /// on it is unavailable. Connected code is analysed in `temp_dir`, the
    /// system temporary directory, and the graphs it is found in are cached
    /// in `cache_dir`, the learner's cache directory, which it creates if it
    /// can. The outbound review waits for connected code for Whetstone's
    /// time limit.
    #[must_use]
    pub fn new(
        theme: Theme,
        settings: Settings,
        showcase: ShowcaseProject,
        repository: Result<Repository, Unavailable>,
        temp_dir: PathBuf,
        cache_dir: &Path,
    ) -> Self {
        let home = Home::new(repository.as_ref());
        let analysis = AnalysisDirs::new(temp_dir);
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
            jobs: Jobs::new(),
            ticks: 0,
            _analysis_guard: analysis.guard(),
            analysis,
            cache: Arc::new(GraphCache::open(cache_dir, GraphCache::CAP)),
            time_limit: connected_code::TIME_LIMIT,
            ai: None,
            refused_models: Arc::default(),
            store: Box::new(InMemoryStore::new()),
        }
    }

    /// The same app, except that the outbound review waits `time_limit` for
    /// connected code, as a test sets it.
    #[must_use]
    pub fn with_time_limit(self, time_limit: Duration) -> Self {
        Self { time_limit, ..self }
    }

    /// The same app, with Practice asking Jev and the generator through
    /// `ai`. Without one, Practice cannot start: Generate questions on the
    /// showcase project is disabled.
    #[must_use]
    pub fn with_ai(self, ai: Arc<dyn Ai>) -> Self {
        Self {
            ai: Some(ai),
            ..self
        }
    }

    /// The same app, keeping session records in `store` instead of in a
    /// store of its own in memory.
    #[must_use]
    pub fn with_store(self, store: Box<dyn SessionStore>) -> Self {
        Self { store, ..self }
    }

    /// Applies one message. Key presses act, a paste goes into the focused
    /// text entry, and a resize changes only how the next frame is laid
    /// out. A job's result is applied only if the app still waits for that
    /// job.
    pub fn handle(&mut self, message: Message) -> Flow {
        match message {
            Message::Terminal(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                self.press(&key)
            }
            Message::Terminal(Event::Paste(text)) => {
                self.paste(&text);
                Flow::Continue
            }
            Message::Terminal(Event::Resize(width, height)) => {
                self.viewport = Some(Size::new(width, height));
                Flow::Continue
            }
            Message::Finished(finished) => {
                self.apply_job(finished);
                Flow::Continue
            }
            Message::TimeUp(job) => {
                self.time_up(job);
                Flow::Continue
            }
            Message::Tick => {
                self.ticks = self.ticks.wrapping_add(1);
                Flow::Continue
            }
            Message::Terminal(_) => Flow::Continue,
        }
    }

    /// The background jobs the app started since the last take, for the UI
    /// loop to run.
    pub fn take_jobs(&mut self) -> Vec<Job<JobOutput>> {
        self.jobs.take_started()
    }

    /// The time limits the app set on its jobs since the last take, for the
    /// UI loop to time.
    pub fn take_time_limits(&mut self) -> Vec<TimeLimit> {
        self.jobs.take_time_limits()
    }

    /// Rule 24: applies a job's result only if the app still waits for that
    /// job.
    fn apply_job(&mut self, finished: Finished<JobOutput>) {
        let Some(finished) = self.jobs.accept(finished) else {
            return;
        };
        match finished.outcome {
            JobOutcome::Done(output) => {
                if let Some(screen) = self.screens.waiting_for(finished.id) {
                    screen.finish_job(finished.id, output);
                    screen.keep_record(self.store.as_mut());
                    screen.start_jobs(&mut self.jobs);
                } else {
                    // The screen that started it has closed, so its result
                    // is for nothing that is open.
                    tracing::info!("a background job ended after its screen closed");
                }
            }
            // A job the app waits for is cancelled only as the app quits.
            JobOutcome::Cancelled => {}
        }
    }

    /// Gives up waiting for the job `job`, whose time limit ran out, unless
    /// it ended in time. It is not cancelled: it runs to its end, and its
    /// result is ignored when it comes.
    fn time_up(&mut self, job: JobId) {
        if !self.jobs.forget(job) {
            return;
        }
        if let Some(screen) = self.screens.waiting_for(job) {
            screen.time_up(job);
            screen.keep_record(self.store.as_mut());
            screen.start_jobs(&mut self.jobs);
        }
    }

    fn press(&mut self, key: &KeyEvent) -> Flow {
        if let Some(command) = keymap::command(keymap::RESERVED, key) {
            return self.perform(command);
        }
        // Under the notice nothing is visible, so no key changes anything.
        if self.shows_notice() {
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
                Outcome::Leave(command) => {
                    self.overlay = None;
                    self.perform(command)
                }
            };
        }
        let screen = self.screens.current();
        let command = match screen.input() {
            // Rule 29: while a text entry has focus, every key but the
            // reserved ones types, before any binding.
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

    /// Whether the window is too small for the screens, so the notice
    /// shows instead.
    fn shows_notice(&self) -> bool {
        self.viewport.is_some_and(|size| !fits(size))
    }

    /// Rule 57: a paste is text for the focused text entry, never keys, so
    /// it skips the keymap. Under the notice, under an overlay, which has no
    /// text entry, or on a screen without a focused one, it does nothing.
    fn paste(&mut self, text: &str) {
        if self.shows_notice() || self.overlay.is_some() {
            return;
        }
        let screen = self.screens.current_mut();
        if screen.input() == Input::Text {
            screen.paste(text);
        }
    }

    /// Runs `command`, unless it would lose a draft: then it asks first.
    fn run(&mut self, command: Command) -> Flow {
        if self.loses_draft(command) {
            self.overlay = Some(Overlay::Leave(Leave::new(command)));
            return Flow::Continue;
        }
        self.perform(command)
    }

    /// Whether `command` would close a screen holding a draft: Back closes
    /// the current screen, and going home or quitting closes every screen
    /// above home.
    fn loses_draft(&self, command: Command) -> bool {
        match command {
            Command::Back => self.screens.current().holds_draft(),
            Command::Go(Route::Home) | Command::Quit => {
                self.screens.above.iter().any(|screen| screen.holds_draft())
            }
            _ => false,
        }
    }

    /// Runs `command` without asking.
    fn perform(&mut self, command: Command) -> Flow {
        match command {
            Command::Quit => return Flow::Quit,
            Command::Suspend => return Flow::Suspend,
            Command::KeyHelp => {
                self.overlay = Some(Overlay::KeyHelp(KeyHelp::new(self.screens.current())));
            }
            Command::Palette => {
                self.overlay = Some(Overlay::Palette(Palette::new(self.screens.current())));
            }
            Command::Go(route) | Command::Replace(route) => {
                let context = Context {
                    theme: &self.theme,
                    settings: &self.settings,
                    showcase: &self.showcase,
                    repository: self.repository.as_ref(),
                    analysis: &self.analysis,
                    ticks: self.ticks,
                    cache: &self.cache,
                    time_limit: self.time_limit,
                    ai: self.ai.as_ref(),
                    refused_models: &self.refused_models,
                };
                if let Command::Replace(_) = command {
                    self.screens.replace(route, &context, &mut self.jobs);
                } else {
                    self.screens.go(route, &context, &mut self.jobs);
                }
            }
            Command::Back => self.screens.back(&mut self.jobs),
            command => {
                // Until the loop reports a size, assume the smallest the
                // screens are drawn in.
                let size = self.viewport.unwrap_or(MINIMUM);
                let [area, _] = screen_and_footer(Rect::from((Position::ORIGIN, size)));
                let screen = self.screens.current_mut();
                let next = screen.apply(command, area);
                screen.keep_record(self.store.as_mut());
                screen.start_jobs(&mut self.jobs);
                if let Some(next) = next {
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
            analysis: &self.analysis,
            ticks: self.ticks,
            cache: &self.cache,
            time_limit: self.time_limit,
            ai: self.ai.as_ref(),
            refused_models: &self.refused_models,
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
