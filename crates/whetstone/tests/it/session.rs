//! The app seam's harness: build the app from injected inputs, then drive it
//! the way the UI loop does, with key presses and resizes, drawing each frame
//! on ratatui's test backend. Background jobs' results and ticks arrive only
//! when a test asks, so every frame is deterministic.

use std::fs;
use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;
use whetstone::{App, Dirs, Environment, Flow, Message, Repository, Settings, Theme};
use whetstone_core::ShowcaseProject;

/// What the app starts from: a global config file, if any, the environment
/// variables besides `HOME`, the showcase project, and the directory
/// Whetstone starts in.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Setup<'a> {
    /// The contents of `config.toml` in the global config directory.
    pub(crate) config: Option<&'a str>,
    pub(crate) variables: &'a [(&'a str, &'a str)],
    /// The showcase project to practice on, if not the embedded one.
    pub(crate) showcase: Option<&'a ShowcaseProject>,
    /// The directory Whetstone starts in, where it looks for the learner's
    /// repository. Without one it starts in the session's home directory,
    /// outside any repository.
    pub(crate) start: Option<&'a Path>,
}

/// A terminal with colour turned off: snapshots then list only text
/// attributes, so they stay short.
pub(crate) const NO_COLOR: &[(&str, &str)] = &[("NO_COLOR", "1")];

/// A running app in a terminal of a given size. Its home directory is a
/// fresh temporary directory, so the real one is never read.
#[derive(Debug)]
pub(crate) struct Session {
    app: App,
    width: u16,
    height: u16,
    _home: TempDir,
}

impl Session {
    /// Starts the app as `setup` says in a `width` by `height` terminal.
    pub(crate) fn start(setup: Setup<'_>, width: u16, height: u16) -> Self {
        let home = TempDir::new().expect("create a temporary home directory");
        let environment: Environment = setup
            .variables
            .iter()
            .copied()
            .chain([("HOME", home.path().to_str().expect("a UTF-8 path"))])
            .collect();
        let dirs = Dirs::resolve(&environment).expect("the home directory is absolute");
        if let Some(config) = setup.config {
            fs::create_dir_all(&dirs.config).expect("create the config directory");
            fs::write(dirs.config.join("config.toml"), config).expect("write the config file");
        }
        let settings = Settings::load(&dirs.config, &environment).expect("the config loads");
        let theme = Theme::new(settings.color_support());
        let repository = Repository::open(setup.start.unwrap_or(home.path()));
        let mut session = Self {
            app: App::new(
                theme,
                settings,
                setup
                    .showcase
                    .cloned()
                    .unwrap_or_else(whetstone::embedded_showcase),
                repository,
            ),
            width,
            height,
            _home: home,
        };
        // The UI loop tells the app the terminal's size before the first draw.
        session.resize(width, height);
        session
    }

    /// Starts the app without a config file or colour.
    pub(crate) fn plain(width: u16, height: u16) -> Self {
        Self::start(
            Setup {
                variables: NO_COLOR,
                ..Setup::default()
            },
            width,
            height,
        )
    }

    /// Presses each key in `script`, separated by spaces: a character such
    /// as `j`, `?` or `:`, or a named key such as `<Enter>`, `<Space>`,
    /// `<Tab>`, `<S-Tab>`, `<Esc>`, `<BS>` (Backspace), `<Up>`, `<Down>`,
    /// `<Left>`, `<Right>`, `<C-c>`, `<C-d>`, `<C-u>` or `<C-z>`. Returns
    /// what the last key asked of the loop, and stops at a quit, as the loop
    /// does. After a suspend the loop carries on, so keys can follow it.
    pub(crate) fn press(&mut self, script: &str) -> Flow {
        let mut flow = Flow::Continue;
        for name in script.split_whitespace() {
            assert_ne!(flow, Flow::Quit, "{name} pressed after the app quit");
            flow = self.app.handle(Message::Terminal(Event::Key(key(name))));
        }
        flow
    }

    /// Applies one terminal event as it arrives, whatever it is.
    pub(crate) fn handle(&mut self, event: &Event) -> Flow {
        self.app.handle(Message::Terminal(event.clone()))
    }

    /// Resizes the terminal, as the UI loop reports it.
    pub(crate) fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        assert_eq!(
            self.app
                .handle(Message::Terminal(Event::Resize(width, height))),
            Flow::Continue,
            "a resize never quits"
        );
    }

    /// Runs every background job the app has started, including any it
    /// starts on a result, and delivers each result, as the UI loop does
    /// once each job ends. Until a test settles, no job runs, so a frame
    /// shows the app still waiting.
    pub(crate) fn settle(&mut self) {
        loop {
            let jobs = self.app.take_jobs();
            if jobs.is_empty() {
                return;
            }
            for job in jobs {
                assert_eq!(
                    self.app.handle(Message::Finished(job.run())),
                    Flow::Continue,
                    "a job's result never quits"
                );
            }
        }
    }

    /// Ticks once, as the UI loop does while a job runs.
    pub(crate) fn tick(&mut self) {
        assert_eq!(
            self.app.handle(Message::Tick),
            Flow::Continue,
            "a tick never quits"
        );
    }

    /// Draws the current frame. Its debug form lists every style change
    /// next to the text, so snapshots show colour and attributes too.
    pub(crate) fn frame(&self) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(self.width, self.height))
            .expect("create a test terminal");
        terminal
            .draw(|frame| self.app.render(frame))
            .expect("draw a frame");
        terminal.backend().buffer().clone()
    }

    /// The current frame's text, one string per row.
    pub(crate) fn rows(&self) -> Vec<String> {
        let frame = self.frame();
        frame
            .content()
            .chunks(usize::from(frame.area.width))
            .map(|row| row.iter().map(Cell::symbol).collect())
            .collect()
    }

    /// The current frame's text as one string.
    pub(crate) fn text(&self) -> String {
        self.rows().join("\n")
    }

    /// The rows inside the overlay's box, between its side borders, trimmed.
    pub(crate) fn overlay_rows(&self) -> Vec<String> {
        self.rows()
            .iter()
            .filter_map(|row| {
                let (_, inside) = row.split_once('│')?;
                let (inside, _) = inside.rsplit_once('│')?;
                Some(inside.trim().to_owned())
            })
            .collect()
    }

    /// The rows inside the box whose top border starts with `title`, between
    /// its side borders, trimmed. Unlike [`Session::overlay_rows`], it finds
    /// the box among others side by side, such as the change viewer's
    /// panes.
    pub(crate) fn box_rows(&self, title: &str) -> Vec<String> {
        let rows: Vec<Vec<char>> = self
            .rows()
            .iter()
            .map(|row| row.chars().collect())
            .collect();
        let corner: Vec<char> = format!("┌ {title}").chars().collect();
        let top = rows.iter().enumerate().find_map(|(y, row)| {
            let x = row
                .windows(corner.len())
                .position(|cells| cells == corner)?;
            let width = row.iter().skip(x).position(|&cell| cell == '┐')?;
            Some((y, x, width))
        });
        assert!(top.is_some(), "no box titled {title:?}:\n{}", self.text());
        let (y, x, width) = top.unwrap_or_default();
        rows.iter()
            .skip(y + 1)
            .take_while(|row| row.get(x) == Some(&'│'))
            .map(|row| {
                row.iter()
                    .skip(x + 1)
                    .take(width - 1)
                    .collect::<String>()
                    .trim()
                    .to_owned()
            })
            .collect()
    }

    /// The rows under `heading`, trimmed, up to the next blank row.
    pub(crate) fn section(&self, heading: &str) -> Vec<String> {
        let rows = self.rows();
        let start = rows.iter().position(|row| row.trim() == heading);
        assert!(start.is_some(), "no {heading:?} heading:\n{}", self.text());
        rows.iter()
            .skip(start.unwrap_or_default() + 1)
            .map(|row| row.trim().to_owned())
            .take_while(|row| !row.is_empty())
            .collect()
    }

    /// The row showing `needle`, trimmed.
    pub(crate) fn row_with(&self, needle: &str) -> String {
        let rows = self.rows();
        let found = rows.iter().find(|row| row.contains(needle));
        assert!(found.is_some(), "no row shows {needle:?}:\n{}", self.text());
        found.map(|row| row.trim().to_owned()).unwrap_or_default()
    }
}

/// Each of `rows` split into its columns, which are at least two cells
/// apart.
pub(crate) fn columns(rows: &[String]) -> Vec<Vec<&str>> {
    rows.iter()
        .map(|row| {
            row.split("  ")
                .map(str::trim)
                .filter(|column| !column.is_empty())
                .collect()
        })
        .collect()
}

fn key(name: &str) -> KeyEvent {
    let (code, modifiers) = match name {
        "<Enter>" => (KeyCode::Enter, KeyModifiers::NONE),
        "<Space>" => (KeyCode::Char(' '), KeyModifiers::NONE),
        "<Tab>" => (KeyCode::Tab, KeyModifiers::NONE),
        // Terminals report Shift+Tab as a back tab with Shift held.
        "<S-Tab>" => (KeyCode::BackTab, KeyModifiers::SHIFT),
        "<Esc>" => (KeyCode::Esc, KeyModifiers::NONE),
        "<BS>" => (KeyCode::Backspace, KeyModifiers::NONE),
        "<Up>" => (KeyCode::Up, KeyModifiers::NONE),
        "<Down>" => (KeyCode::Down, KeyModifiers::NONE),
        "<Left>" => (KeyCode::Left, KeyModifiers::NONE),
        "<Right>" => (KeyCode::Right, KeyModifiers::NONE),
        "<C-c>" => (KeyCode::Char('c'), KeyModifiers::CONTROL),
        "<C-d>" => (KeyCode::Char('d'), KeyModifiers::CONTROL),
        "<C-u>" => (KeyCode::Char('u'), KeyModifiers::CONTROL),
        "<C-z>" => (KeyCode::Char('z'), KeyModifiers::CONTROL),
        // Terminals report `?` with Shift held.
        "?" => (KeyCode::Char('?'), KeyModifiers::SHIFT),
        _ => {
            let mut chars = name.chars();
            let c = chars.next().expect("key names are not empty");
            assert!(chars.next().is_none(), "unknown key {name:?}");
            (KeyCode::Char(c), KeyModifiers::NONE)
        }
    };
    KeyEvent::new(code, modifiers)
}
