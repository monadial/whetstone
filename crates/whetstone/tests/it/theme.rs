//! The theme layer: how much colour the environment allows, and how controls
//! show their state when colour is limited or absent.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget as _;
use whetstone::{ColorSupport, ControlKind, ControlState, Theme};

use crate::colors::{ansi256_token_colors, colors_used, dark_token_colors, environment};

fn detect(variables: &[(&str, &str)]) -> ColorSupport {
    ColorSupport::detect(&environment(variables))
}

#[test]
fn colour_level_follows_no_color_then_colorterm_then_term() {
    for (variables, expected) in [
        (
            &[("NO_COLOR", "1"), ("COLORTERM", "truecolor")][..],
            ColorSupport::NoColor,
        ),
        (
            &[("NO_COLOR", "0"), ("TERM", "xterm-256color")],
            ColorSupport::NoColor,
        ),
        // An empty NO_COLOR does not count (no-color.org).
        (
            &[("NO_COLOR", ""), ("COLORTERM", "truecolor")],
            ColorSupport::TrueColor,
        ),
        (&[("COLORTERM", "truecolor")], ColorSupport::TrueColor),
        (
            &[("COLORTERM", "24bit"), ("TERM", "xterm")],
            ColorSupport::TrueColor,
        ),
        (
            &[("COLORTERM", "yes"), ("TERM", "xterm-256color")],
            ColorSupport::Ansi256,
        ),
        (&[("TERM", "tmux-256color")], ColorSupport::Ansi256),
        (&[("TERM", "xterm")], ColorSupport::Ansi16),
        (&[], ColorSupport::Ansi16),
    ] {
        assert_eq!(detect(variables), expected, "with {variables:?}");
    }
}

const LEVELS: [ColorSupport; 4] = [
    ColorSupport::TrueColor,
    ColorSupport::Ansi256,
    ColorSupport::Ansi16,
    ColorSupport::NoColor,
];

const UNAVAILABLE: Option<&str> = Some("arrives with Jev");

/// Every state a control can be in.
fn all_states() -> Vec<ControlState<'static>> {
    let mut states = Vec::new();
    for focused in [false, true] {
        for kind in [
            ControlKind::Action,
            ControlKind::Choice { selected: false },
            ControlKind::Choice { selected: true },
        ] {
            for disabled in [None, UNAVAILABLE] {
                states.push(ControlState {
                    focused,
                    kind,
                    disabled,
                });
            }
        }
    }
    states
}

/// Draws the control `Domain` in `state` on a one-row buffer.
fn draw_control(color_support: ColorSupport, state: ControlState<'_>) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
    Theme::new(color_support)
        .control("Domain", state)
        .render(buffer.area, &mut buffer);
    buffer
}

/// A control as it reads without colour: runs of text, each with its text
/// attributes, trailing blanks dropped.
fn reading(color_support: ColorSupport, state: ControlState<'_>) -> Vec<(String, Modifier)> {
    let mut runs: Vec<(String, Modifier)> = Vec::new();
    for cell in draw_control(color_support, state).content() {
        match runs.last_mut() {
            Some((text, modifier)) if *modifier == cell.modifier => text.push_str(cell.symbol()),
            _ => runs.push((cell.symbol().to_owned(), cell.modifier)),
        }
    }
    if let Some((text, _)) = runs.last_mut() {
        text.truncate(text.trim_end().len());
    }
    runs.retain(|(text, _)| !text.is_empty());
    runs
}

/// A control's text alone, as a terminal that ignores attributes shows it.
fn text(state: ControlState<'_>) -> String {
    reading(ColorSupport::NoColor, state)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

fn run(text: &str, modifier: Modifier) -> (String, Modifier) {
    (text.to_owned(), modifier)
}

#[test]
fn without_colour_markers_and_attributes_show_focus_selection_and_disabled() {
    let none = Modifier::empty();
    let disabled = Modifier::DIM | Modifier::ITALIC;
    let action = ControlState::default();
    for (state, expected) in [
        (action, vec![run("  Domain", none)]),
        (
            ControlState {
                focused: true,
                ..action
            },
            vec![run("> Domain", Modifier::BOLD)],
        ),
        (
            ControlState {
                kind: ControlKind::Choice { selected: false },
                ..action
            },
            vec![run("  ( ) Domain", none)],
        ),
        (
            ControlState {
                kind: ControlKind::Choice { selected: true },
                ..action
            },
            vec![run("  (x) Domain", none)],
        ),
        (
            ControlState {
                focused: true,
                kind: ControlKind::Choice { selected: true },
                ..action
            },
            vec![run("> (x) Domain", Modifier::BOLD)],
        ),
        (
            ControlState {
                disabled: UNAVAILABLE,
                ..action
            },
            vec![run("  ", none), run("Domain (arrives with Jev)", disabled)],
        ),
        (
            ControlState {
                focused: true,
                disabled: UNAVAILABLE,
                ..action
            },
            vec![
                run("> ", Modifier::BOLD),
                run("Domain (arrives with Jev)", disabled),
            ],
        ),
    ] {
        assert_eq!(reading(ColorSupport::NoColor, state), expected, "{state:?}");
    }
}

#[test]
fn without_colour_no_control_state_is_drawn_in_colour() {
    for state in all_states() {
        let buffer = draw_control(ColorSupport::NoColor, state);

        assert!(
            buffer
                .content()
                .iter()
                .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset),
            "{state:?} is drawn in terminal colours only: {buffer:?}"
        );
    }
}

#[test]
fn every_control_state_reads_differently_by_its_text_alone() {
    let states = all_states();
    for (i, first) in states.iter().enumerate() {
        for second in &states[i + 1..] {
            assert_ne!(text(*first), text(*second), "{first:?} and {second:?}");
        }
    }
}

#[test]
fn at_every_colour_level_colour_only_adds_to_the_markers_and_attributes() {
    for color_support in LEVELS {
        for state in all_states() {
            assert_eq!(
                reading(color_support, state),
                reading(ColorSupport::NoColor, state),
                "{state:?} at {color_support:?}"
            );
        }
    }
}

#[test]
fn controls_in_colour_use_only_the_brand_tokens() {
    for (color_support, tokens) in [
        (ColorSupport::TrueColor, dark_token_colors()),
        (ColorSupport::Ansi256, ansi256_token_colors()),
    ] {
        for state in all_states() {
            // The canvas comes from the screen behind the control.
            let used: HashSet<Color> = colors_used(&draw_control(color_support, state))
                .into_iter()
                .filter(|color| *color != Color::Reset)
                .collect();

            assert!(
                used.is_subset(&tokens),
                "{state:?} at {color_support:?}: {used:?} within {tokens:?}"
            );
        }
    }
}
