//! App seam: build the app from injected inputs, feed it terminal events and
//! snapshot the frames it draws on ratatui's test backend.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use whetstone::{App, Flow, Theme};

fn draw(app: &App, width: u16, height: u16) -> TestBackend {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("create a test terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("draw a frame");
    terminal.backend().clone()
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

#[test]
fn home_lists_practice_settings_and_quit_at_120x32() {
    let app = App::new(Theme::plain());

    insta::assert_snapshot!("home_120x32", draw(&app, 120, 32));
}

#[test]
fn home_lists_practice_settings_and_quit_at_80x24() {
    let app = App::new(Theme::plain());

    insta::assert_snapshot!("home_80x24", draw(&app, 80, 24));
}

#[test]
fn q_quits_from_home() {
    let mut app = App::new(Theme::plain());

    assert_eq!(
        app.handle(&key(KeyCode::Char('q'), KeyModifiers::NONE)),
        Flow::Quit
    );
}

#[test]
fn ctrl_c_quits_from_home() {
    let mut app = App::new(Theme::plain());

    assert_eq!(
        app.handle(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Flow::Quit
    );
}

#[test]
fn other_keys_keep_the_app_running() {
    let mut app = App::new(Theme::plain());

    for event in [
        key(KeyCode::Char('c'), KeyModifiers::NONE),
        key(KeyCode::Char('x'), KeyModifiers::NONE),
        key(KeyCode::Esc, KeyModifiers::NONE),
        Event::Resize(80, 24),
        // Only key presses act; a release of `q` is not a second quit.
        Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
    ] {
        assert_eq!(app.handle(&event), Flow::Continue, "after {event:?}");
    }
}
