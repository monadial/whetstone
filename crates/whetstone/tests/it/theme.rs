//! The theme layer: how much colour the environment allows, and how controls
//! show their state when colour is limited or absent.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget as _;
use whetstone::{ColorSupport, ControlState, Environment, Theme};

fn detect(variables: &[(&str, &str)]) -> ColorSupport {
    let environment: Environment = variables.iter().copied().collect();
    ColorSupport::detect(&environment)
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

const ACTION: ControlState = ControlState {
    focused: false,
    selected: None,
    disabled: false,
};

/// Every state a control can be in.
fn all_states() -> Vec<ControlState> {
    let mut states = Vec::new();
    for focused in [false, true] {
        for selected in [None, Some(false), Some(true)] {
            for disabled in [false, true] {
                states.push(ControlState {
                    focused,
                    selected,
                    disabled,
                });
            }
        }
    }
    states
}

/// A control as it reads without colour: runs of text, each with its text
/// attributes, trailing blanks dropped.
fn reading(colors: ColorSupport, state: ControlState) -> Vec<(String, Modifier)> {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
    Theme::new(colors)
        .control("Domain", state)
        .render(buffer.area, &mut buffer);
    let mut runs: Vec<(String, Modifier)> = Vec::new();
    for cell in buffer.content() {
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

fn run(text: &str, modifier: Modifier) -> (String, Modifier) {
    (text.to_owned(), modifier)
}

#[test]
fn without_colour_markers_and_attributes_show_focus_selection_and_disabled() {
    let none = Modifier::empty();
    let disabled = Modifier::DIM | Modifier::ITALIC;
    for (state, expected) in [
        (ACTION, vec![run("  Domain", none)]),
        (
            ControlState {
                focused: true,
                ..ACTION
            },
            vec![run("> Domain", Modifier::BOLD)],
        ),
        (
            ControlState {
                selected: Some(false),
                ..ACTION
            },
            vec![run("  ( ) Domain", none)],
        ),
        (
            ControlState {
                selected: Some(true),
                ..ACTION
            },
            vec![run("  (x) Domain", none)],
        ),
        (
            ControlState {
                focused: true,
                selected: Some(true),
                ..ACTION
            },
            vec![run("> (x) Domain", Modifier::BOLD)],
        ),
        (
            ControlState {
                disabled: true,
                ..ACTION
            },
            vec![run("  ", none), run("Domain", disabled)],
        ),
        (
            ControlState {
                focused: true,
                disabled: true,
                ..ACTION
            },
            vec![run("> ", Modifier::BOLD), run("Domain", disabled)],
        ),
    ] {
        assert_eq!(reading(ColorSupport::NoColor, state), expected, "{state:?}");
    }
}

#[test]
fn without_colour_no_control_state_is_drawn_in_colour() {
    for state in all_states() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
        Theme::new(ColorSupport::NoColor)
            .control("Domain", state)
            .render(buffer.area, &mut buffer);

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
fn every_control_state_reads_differently_without_colour() {
    let states = all_states();
    for (i, first) in states.iter().enumerate() {
        for second in &states[i + 1..] {
            assert_ne!(
                reading(ColorSupport::NoColor, *first),
                reading(ColorSupport::NoColor, *second),
                "{first:?} and {second:?}"
            );
        }
    }
}

#[test]
fn at_every_colour_level_colour_only_adds_to_the_markers_and_attributes() {
    for colors in LEVELS {
        for state in all_states() {
            assert_eq!(
                reading(colors, state),
                reading(ColorSupport::NoColor, state),
                "{state:?} at {colors:?}"
            );
        }
    }
}
