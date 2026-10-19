//! The app seam's harness: build the app from injected inputs, then drive it
//! the way the UI loop does, with key presses and resizes, drawing each frame
//! on ratatui's test backend.

use std::fs;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;
use whetstone::{App, ColorSupport, Dirs, Environment, Flow, Settings, Theme};

/// What the app starts from: a global config file, if any, and the
/// environment variables besides `HOME`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Setup<'a> {
    /// The contents of `config.toml` in the global config directory.
    pub(crate) config: Option<&'a str>,
    pub(crate) variables: &'a [(&'a str, &'a str)],
}

/// A terminal with colour turned off: snapshots then list only text
/// attributes, so they stay short.
pub(crate) const NO_COLOR: &[(&str, &str)] = &[("NO_COLOR", "1")];

/// A running app in a terminal of a given size. Its home directory is a
/// fresh temporary directory, so the real one is never read.
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
        let mut session = Self {
            app: App::new(Theme::new(ColorSupport::detect(&environment)), settings),
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
    /// `<Tab>`, `<S-Tab>`, `<Esc>`, `<Up>`, `<Down>` or `<C-c>`. Returns what
    /// the last key asked of the loop, and stops at a quit, as the loop does.
    pub(crate) fn press(&mut self, script: &str) -> Flow {
        let mut flow = Flow::Continue;
        for name in script.split_whitespace() {
            assert_eq!(flow, Flow::Continue, "{name} pressed after the app quit");
            flow = self.app.handle(&Event::Key(key(name)));
        }
        flow
    }

    /// Applies one terminal event as it arrives, whatever it is.
    pub(crate) fn handle(&mut self, event: &Event) -> Flow {
        self.app.handle(event)
    }

    /// Resizes the terminal, as the UI loop reports it.
    pub(crate) fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        assert_eq!(
            self.app.handle(&Event::Resize(width, height)),
            Flow::Continue,
            "a resize never quits"
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

    /// The row showing `needle`, trimmed.
    pub(crate) fn row_with(&self, needle: &str) -> String {
        let rows = self.rows();
        let found = rows.iter().find(|row| row.contains(needle));
        assert!(found.is_some(), "no row shows {needle:?}:\n{}", self.text());
        found.map(|row| row.trim().to_owned()).unwrap_or_default()
    }
}

fn key(name: &str) -> KeyEvent {
    let (code, modifiers) = match name {
        "<Enter>" => (KeyCode::Enter, KeyModifiers::NONE),
        "<Space>" => (KeyCode::Char(' '), KeyModifiers::NONE),
        "<Tab>" => (KeyCode::Tab, KeyModifiers::NONE),
        // Terminals report Shift+Tab as a back tab with Shift held.
        "<S-Tab>" => (KeyCode::BackTab, KeyModifiers::SHIFT),
        "<Esc>" => (KeyCode::Esc, KeyModifiers::NONE),
        "<Up>" => (KeyCode::Up, KeyModifiers::NONE),
        "<Down>" => (KeyCode::Down, KeyModifiers::NONE),
        "<C-c>" => (KeyCode::Char('c'), KeyModifiers::CONTROL),
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
