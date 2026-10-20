//! The row of stages that shows where Practice is and what comes next:
//! `✓ Reading the change → ⠋ Finding connected code → · Preparing evidence`.
//!
//! Each stage's mark says where it stands, so the row reads the same
//! without colour, and styles only add to the marks. There are no
//! percentages or timers. The row wraps only between stages, never inside
//! one.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::columns;
use crate::theme::Theme;

/// Where a stage stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StageState {
    /// Finished: marked `✓`.
    Done,
    /// What Practice is doing now: marked with a spinner.
    Current,
    /// Not reached yet: marked `·` and dimmed.
    Upcoming,
    /// Passed over, such as finding connected code when it isn't available:
    /// marked `–`.
    Skipped,
}

/// One stage in the row: what it is called and where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stage<'a> {
    /// The stage's name, as the row shows it.
    pub(crate) label: &'a str,
    pub(crate) state: StageState,
}

/// The stages of a Practice, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PracticeStage {
    ReadingTheChange,
    FindingConnectedCode,
    PreparingEvidence,
    OutboundReview,
    /// Jev checks the chosen areas against the change.
    CheckingAreas,
    /// The generator writes the questions.
    WritingQuestions,
    /// Jev reviews the generator's questions.
    ReviewingQuestions,
}

impl PracticeStage {
    /// Every stage, in the order Practice goes through them.
    pub(crate) const ALL: [Self; 7] = [
        Self::ReadingTheChange,
        Self::FindingConnectedCode,
        Self::PreparingEvidence,
        Self::OutboundReview,
        Self::CheckingAreas,
        Self::WritingQuestions,
        Self::ReviewingQuestions,
    ];

    /// The stage's name in the row, with who does the work where that is an
    /// AI role.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ReadingTheChange => "Reading the change",
            Self::FindingConnectedCode => "Finding connected code",
            Self::PreparingEvidence => "Preparing evidence",
            Self::OutboundReview => "Outbound review",
            Self::CheckingAreas => "Checking areas · Jev",
            Self::WritingQuestions => "Writing questions · generator",
            Self::ReviewingQuestions => "Reviewing questions · Jev",
        }
    }
}

/// The spinner's frames, one per tick. No frame looks like another state's
/// mark, so the current stage reads as current without colour.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// The mark of a finished stage.
const DONE: &str = "✓";
/// The mark of a stage not reached yet.
const UPCOMING: &str = "·";
/// The mark of a stage passed over.
const SKIPPED: &str = "–";
/// What follows every stage but the last, leading to the next.
const ARROW: &str = " →";

/// The row of stages, as one widget. It draws the stages it is given in
/// order, each as its mark and its label, joined by `→`.
///
/// Drawing is pure: the spinner shows the frame for the tick count it was
/// given, so it moves only when the caller passes a later tick.
///
/// The row keeps the theme's styles rather than the theme, so its lines
/// borrow only the labels and can outlive the theme.
#[derive(Debug, Clone)]
pub(crate) struct StageRow<'a> {
    stages: Vec<Stage<'a>>,
    tick: usize,
    /// An upcoming stage's style.
    upcoming: Style,
    /// A skipped stage's style, and the arrows'.
    muted: Style,
}

impl<'a> StageRow<'a> {
    /// The row of `stages` in `theme`'s styles, with the current stage's
    /// spinner at its frame for `tick`, the number of ticks so far.
    pub(crate) fn new(
        stages: impl IntoIterator<Item = Stage<'a>>,
        tick: usize,
        theme: &Theme,
    ) -> Self {
        Self {
            stages: stages.into_iter().collect(),
            tick,
            upcoming: theme.upcoming_stage,
            muted: theme.muted,
        }
    }

    /// The row laid out on lines `width` cells wide. A line breaks only
    /// between stages, after the arrow.
    pub(crate) fn lines(&self, width: u16) -> Vec<Line<'a>> {
        let width = usize::from(width);
        let mut lines: Vec<Line<'a>> = Vec::new();
        for (index, stage) in self.stages.iter().enumerate() {
            let last = index + 1 == self.stages.len();
            let spans = self.stage(stage, last, width);
            let cells: usize = spans.iter().map(Span::width).sum();
            match lines.last_mut() {
                Some(line) if line.width() + 1 + cells <= width => {
                    line.push_span(" ");
                    line.spans.extend(spans);
                }
                _ => lines.push(Line::from(spans)),
            }
        }
        lines
    }

    /// One stage: its mark, its label, and the arrow to the next unless it is
    /// the `last`. A label too long for a line `width` cells wide is cut, so
    /// that the stage still fits on one line, mark and arrow included.
    fn stage(&self, stage: &Stage<'a>, last: bool, width: usize) -> Vec<Span<'a>> {
        let (mark, style) = match stage.state {
            StageState::Done => (DONE, Style::new()),
            StageState::Current => (SPINNER[self.tick % SPINNER.len()], Style::new()),
            StageState::Upcoming => (UPCOMING, self.upcoming),
            StageState::Skipped => (SKIPPED, self.muted),
        };
        let arrow = if last { "" } else { ARROW };
        let room = width.saturating_sub(Line::raw(format!("{mark} {arrow}")).width());
        vec![
            Span::styled(mark, style),
            Span::styled(" ", style),
            Span::styled(columns::truncate(stage.label, room), style),
            Span::styled(arrow, self.muted),
        ]
    }
}

impl Widget for &StageRow<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines(area.width)).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorSupport;

    use StageState::{Current, Done, Skipped, Upcoming};

    /// Practice's stages in the given states.
    fn practice(states: [StageState; 7]) -> impl Iterator<Item = Stage<'static>> {
        PracticeStage::ALL
            .into_iter()
            .zip(states)
            .map(|(stage, state)| Stage {
                label: stage.label(),
                state,
            })
    }

    /// While connected code is being found.
    const IN_PROGRESS: [StageState; 7] = [
        Done, Current, Upcoming, Upcoming, Upcoming, Upcoming, Upcoming,
    ];
    /// Once the evidence is prepared.
    const AT_THE_OUTBOUND_REVIEW: [StageState; 7] =
        [Done, Done, Done, Current, Upcoming, Upcoming, Upcoming];
    /// Once the evidence is prepared without connected code.
    const CONNECTED_CODE_SKIPPED: [StageState; 7] =
        [Done, Skipped, Done, Current, Upcoming, Upcoming, Upcoming];

    /// `row` drawn `width` cells wide, on as many lines as it is laid out on.
    fn drawn(row: &StageRow<'_>, width: u16) -> Buffer {
        let height = u16::try_from(row.lines(width).len()).expect("the row is a few lines high");
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        row.render(buffer.area, &mut buffer);
        buffer
    }

    /// The text of each line in `buffer`, without the blank cells after it.
    fn text(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// Snapshots the row of Practice's stages in `states` at both snapshot
    /// widths, with colour and without.
    fn assert_snapshots(name: &str, states: [StageState; 7]) {
        for width in [120, 80] {
            for (colour, support) in [
                ("in_colour", ColorSupport::TrueColor),
                ("without_colour", ColorSupport::NoColor),
            ] {
                let theme = Theme::new(support);
                let row = StageRow::new(practice(states), 0, &theme);
                insta::assert_debug_snapshot!(
                    format!("{name}_{colour}_{width}"),
                    drawn(&row, width)
                );
            }
        }
    }

    #[test]
    fn in_progress() {
        assert_snapshots("in_progress", IN_PROGRESS);
    }

    #[test]
    fn done_up_to_the_outbound_review() {
        assert_snapshots("at_the_outbound_review", AT_THE_OUTBOUND_REVIEW);
    }

    #[test]
    fn connected_code_skipped() {
        assert_snapshots("connected_code_skipped", CONNECTED_CODE_SKIPPED);
    }

    #[test]
    fn the_spinner_moves_on_each_tick_and_stands_still_between_them() {
        let theme = Theme::new(ColorSupport::NoColor);
        let first_line = |tick| {
            text(&drawn(
                &StageRow::new(practice(IN_PROGRESS), tick, &theme),
                120,
            ))
            .remove(0)
        };
        assert_eq!(
            first_line(0),
            "✓ Reading the change → ⠋ Finding connected code → · Preparing evidence → · Outbound review → · Checking areas · Jev →"
        );
        // Drawn again at the same tick, it has not moved.
        assert_eq!(first_line(0), first_line(0));
        assert_eq!(
            first_line(1),
            "✓ Reading the change → ⠙ Finding connected code → · Preparing evidence → · Outbound review → · Checking areas · Jev →"
        );
        let spinner = |tick| {
            first_line(tick)
                .split(" → ")
                .nth(1)
                .and_then(|stage| stage.chars().next())
        };
        let frames: Vec<_> = (0..10).map(spinner).collect();
        for (index, frame) in frames.iter().enumerate() {
            assert!(
                !frames[..index].contains(frame),
                "frame {index} shows {frame:?} again"
            );
            assert!(
                !['✓', '·', '–'].map(Some).contains(frame),
                "frame {index} looks like another mark"
            );
        }
        assert_eq!(spinner(10), spinner(0), "the spinner comes round again");
    }

    #[test]
    fn a_narrow_row_wraps_between_stages_never_inside_one() {
        let theme = Theme::new(ColorSupport::NoColor);
        let row = StageRow::new(practice(AT_THE_OUTBOUND_REVIEW), 0, &theme);
        // As narrow as a screen gets: the 50-cell floor less its margins.
        assert_eq!(
            text(&drawn(&row, 46)),
            [
                "✓ Reading the change →",
                "✓ Finding connected code →",
                "✓ Preparing evidence → ⠋ Outbound review →",
                "· Checking areas · Jev →",
                "· Writing questions · generator →",
                "· Reviewing questions · Jev",
            ]
        );
        // Down to the widest stage, "· Writing questions · generator →".
        for width in 33..=120 {
            let lines = text(&drawn(&row, width));
            for stage in PracticeStage::ALL {
                assert_eq!(
                    lines
                        .iter()
                        .filter(|line| line.contains(stage.label()))
                        .count(),
                    1,
                    "{:?} is not whole on one line {width} cells wide: {lines:#?}",
                    stage.label()
                );
            }
            assert!(
                lines.iter().all(|line| !line.is_empty()),
                "{width} cells wide, the row asks for a line it leaves empty: {lines:#?}"
            );
        }
    }

    #[test]
    fn a_stage_wider_than_the_row_is_cut_on_a_line_of_its_own() {
        let theme = Theme::new(ColorSupport::NoColor);
        let row = StageRow::new(
            [
                Stage {
                    label: "Writing questions · generator",
                    state: Upcoming,
                },
                Stage {
                    label: "Next",
                    state: Upcoming,
                },
            ],
            0,
            &theme,
        );
        assert_eq!(text(&drawn(&row, 20)), ["· Writing questio… →", "· Next"]);
    }

    #[test]
    fn its_lines_outlive_the_theme_for_a_screen_to_keep() {
        let lines: Vec<Line<'static>> = {
            let theme = Theme::new(ColorSupport::NoColor);
            StageRow::new(practice(IN_PROGRESS), 0, &theme).lines(80)
        };
        assert_eq!(
            lines.iter().map(Line::to_string).collect::<Vec<_>>(),
            [
                "✓ Reading the change → ⠋ Finding connected code → · Preparing evidence →",
                "· Outbound review → · Checking areas · Jev → · Writing questions · generator →",
                "· Reviewing questions · Jev",
            ]
        );
    }
}
