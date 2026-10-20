//! The reasoning editor: the multi-line text entry a learner writes their
//! reasoning in (rule 58).
//!
//! It edits by grapheme, the character as the learner sees it, so a cursor
//! never lands inside one and erasing takes a whole one (rule 31).

use std::fmt;
use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::columns;
use crate::keymap::{Binding, Command, Keymap, Press};
use crate::paste;
use crate::theme::Theme;

/// The most reasoning a response holds: 4,000 characters, counted in
/// graphemes, as the learner sees them.
pub(crate) const LIMIT: usize = 4_000;

/// The keys of the reasoning editor, while it has focus. Every other key
/// but the reserved ones types (rule 29), and only Esc and Tab leave it.
pub(crate) const KEYMAP: Keymap = &[
    Binding {
        keys: &[(Press::key(KeyCode::Enter), Command::Type('\n'))],
        does: "Start a new line",
        hint: None,
    },
    Binding {
        keys: &[
            (Press::key(KeyCode::Left), Command::Left),
            (Press::key(KeyCode::Right), Command::Right),
            (Press::key(KeyCode::Up), Command::Up),
            (Press::key(KeyCode::Down), Command::Down),
        ],
        does: "Move the cursor",
        hint: None,
    },
    Binding {
        keys: &[
            (Press::key(KeyCode::Home), Command::LineStart),
            (Press::key(KeyCode::End), Command::LineEnd),
        ],
        does: "Go to the start or end of the line",
        hint: None,
    },
    Binding {
        keys: &[
            (Press::key(KeyCode::Backspace), Command::Erase),
            (Press::key(KeyCode::Delete), Command::Delete),
        ],
        does: "Erase the character before or after the cursor",
        hint: None,
    },
    Binding {
        keys: &[(Press::key(KeyCode::Esc), Command::Cancel)],
        does: "Stop editing, keeping the draft",
        hint: Some("Esc stop editing"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Tab), Command::NextControl)],
        does: "Move to Submit response",
        hint: Some("Tab next"),
    },
];

/// The reasoning written so far, and where the cursor is in it.
///
/// Its `Debug` output shows the reasoning's length, never its text.
#[derive(Clone, Default)]
pub(crate) struct ReasoningEditor {
    text: String,
    /// Where the cursor is: the byte offset of a grapheme boundary in
    /// `text`.
    cursor: usize,
    /// What the last edit ran into, until the next edit.
    notice: Option<Notice>,
}

/// What an edit ran into at the limit, shown below the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Notice {
    /// A paste was cut at the limit, and this many of its graphemes were
    /// left out.
    PasteCut { left_out: usize },
    /// A typed character didn't go in: the reasoning is at the limit.
    AtLimit,
}

/// What the editor asks of its screen after a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Editing {
    /// Keep focus on the editor.
    Stay,
    /// Esc: stop editing. The draft stays as it is.
    Stop,
    /// Tab: leave the editor, towards **Submit response**.
    Next,
}

/// Whether the editor draws its caret: only while the learner is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Caret {
    Shown,
    Hidden,
}

/// The editor as one widget, drawn in the theme's styles.
///
/// It lays the text out for the area it is drawn in, every time (rule 32):
/// lines wrap between words to the area's width, keeping a row's last cell
/// for the caret, and the rows shown follow the cursor.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EditorView<'a> {
    editor: &'a ReasoningEditor,
    caret: Caret,
    /// The caret's style: reversed, so it shows without colour.
    cursor: Style,
}

/// How many cells a tab takes. It shows as that many spaces.
const TAB: usize = 4;

impl ReasoningEditor {
    /// Applies `command` to the editor drawn `width` cells wide: a typed
    /// character goes in at the cursor, Enter's line break too, and the
    /// arrows, Home and End move the cursor. Esc and Tab leave the editor,
    /// and every other command does nothing.
    pub(crate) fn apply(&mut self, command: Command, width: u16) -> Editing {
        match command {
            Command::Type(c) => {
                let left_out = self.insert(c.encode_utf8(&mut [0; 4]));
                self.notice = (left_out > 0).then_some(Notice::AtLimit);
            }
            Command::Erase => {
                if let Some(start) = self.before() {
                    self.text.replace_range(start..self.cursor, "");
                    self.cursor = start;
                }
                self.notice = None;
            }
            Command::Delete => {
                if let Some(end) = self.after() {
                    self.text.replace_range(self.cursor..end, "");
                }
                self.notice = None;
            }
            Command::Up | Command::Down => {
                let rows = rows(&self.text, width);
                let row = row_of(&rows, self.cursor);
                let target = match command {
                    Command::Up => row.checked_sub(1),
                    _ => Some(row + 1).filter(|&below| below < rows.len()),
                };
                if let Some(target) = target {
                    let x = cells(
                        self.text
                            .get(rows[row].start..self.cursor)
                            .unwrap_or_default(),
                    );
                    self.cursor = self.at_column(&rows, target, x);
                }
            }
            Command::Left => self.cursor = self.before().unwrap_or(self.cursor),
            Command::Right => self.cursor = self.after().unwrap_or(self.cursor),
            Command::LineStart => {
                self.cursor = self
                    .text
                    .get(..self.cursor)
                    .and_then(|before| before.rfind('\n'))
                    .map_or(0, |at| at + 1);
            }
            Command::LineEnd => {
                self.cursor = self
                    .text
                    .get(self.cursor..)
                    .and_then(|after| after.find('\n'))
                    .map_or(self.text.len(), |at| self.cursor + at);
            }
            Command::Cancel => return Editing::Stop,
            Command::NextControl => return Editing::Next,
            _ => {}
        }
        Editing::Stay
    }

    /// Inserts pasted `text` at the cursor, as one insertion, and puts the
    /// cursor after it. Its line breaks and tabs are kept, and other control
    /// characters dropped (rule 57). A paste that would take the reasoning
    /// past [`LIMIT`] is cut there, with a notice until the next edit.
    pub(crate) fn paste(&mut self, text: &str) {
        let left_out = self.insert(&paste::clean(text));
        self.notice = (left_out > 0).then_some(Notice::PasteCut { left_out });
    }

    /// The reasoning written so far.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The editor as a widget in `theme`'s styles, with its caret shown or
    /// hidden.
    pub(crate) fn view(&self, theme: &Theme, caret: Caret) -> EditorView<'_> {
        EditorView {
            editor: self,
            caret,
            cursor: theme.cursor,
        }
    }

    /// The place in row `row` of `rows` closest to `x` cells in from its
    /// start, without going past it. A row that wraps onto the next ends
    /// before its last grapheme: the place after it shows at the start of
    /// the next row.
    fn at_column(&self, rows: &[Range<usize>], row: usize, x: usize) -> usize {
        let Range { start, end } = rows[row].clone();
        let wraps = rows.get(row + 1).is_some_and(|next| next.start == end);
        let mut at = start;
        let mut used = 0;
        for (offset, grapheme) in self
            .text
            .get(start..end)
            .unwrap_or_default()
            .grapheme_indices(true)
        {
            let next = start + offset + grapheme.len();
            used += cells(grapheme);
            if used > x || (wraps && next == end) {
                break;
            }
            at = next;
        }
        at
    }

    /// Inserts as much of `text` at the cursor as [`LIMIT`] leaves room
    /// for, whole graphemes from its start, and puts the cursor after it.
    /// Returns how many of its graphemes were left out.
    fn insert(&mut self, text: &str) -> usize {
        let graphemes: Vec<&str> = text.graphemes(true).collect();
        let room = LIMIT.saturating_sub(self.text.graphemes(true).count());
        let mut taken = graphemes.len().min(room);
        // A grapheme can join the text around it, but never splits it, so
        // what fits the room fits the limit. Count to be sure.
        while taken > 0 {
            let piece = graphemes[..taken].concat();
            let mut text = self.text.clone();
            text.insert_str(self.cursor, &piece);
            if text.graphemes(true).count() <= LIMIT {
                self.text = text;
                self.cursor += piece.len();
                self.snap();
                break;
            }
            taken -= 1;
        }
        graphemes.len() - taken
    }

    /// Where the grapheme before the cursor starts, if there is one.
    fn before(&self) -> Option<usize> {
        self.boundaries().rev().find(|&at| at < self.cursor)
    }

    /// Where the grapheme after the cursor ends, if there is one.
    fn after(&self) -> Option<usize> {
        self.boundaries().find(|&at| at > self.cursor)
    }

    /// Moves the cursor forward to the next grapheme boundary, if what was
    /// inserted joined the grapheme after it, as a zero-width joiner does.
    fn snap(&mut self) {
        let cursor = self.cursor;
        let snapped = self.boundaries().find(|&at| at >= cursor);
        self.cursor = snapped.unwrap_or(cursor);
    }

    /// The grapheme boundaries in the text, in order, from its start to its
    /// end.
    fn boundaries(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        self.text
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .chain([self.text.len()])
    }
}

impl Widget for EditorView<'_> {
    fn render(self, mut area: Rect, buf: &mut Buffer) {
        if let Some(notice) = self.editor.notice {
            // The notice takes the rows below the text, leaving it one.
            let lines = columns::wrap(&notice.to_string(), usize::from(area.width));
            let height = u16::try_from(lines.len())
                .unwrap_or(u16::MAX)
                .min(area.height.saturating_sub(1));
            let [text, below] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(height)]).areas(area);
            Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
                .render(below, buf);
            area = text;
        }
        let text = &self.editor.text;
        let cursor = self.editor.cursor;
        let rows = rows(text, area.width);
        let height = usize::from(area.height);
        let row = row_of(&rows, cursor);
        // The cursor's row in the middle while it can be, and the rows
        // filled to the last.
        let first = row
            .saturating_sub(height / 2)
            .min(rows.len().saturating_sub(height));
        let lines: Vec<Line<'_>> = rows
            .iter()
            .enumerate()
            .skip(first)
            .take(height)
            .map(|(index, range)| {
                let row_text = text.get(range.clone()).unwrap_or_default();
                if index != row || self.caret == Caret::Hidden {
                    return Line::raw(shown(row_text));
                }
                let before = text.get(range.start..cursor).unwrap_or_default();
                let at = text
                    .get(cursor..range.end)
                    .and_then(|after| after.graphemes(true).next())
                    .unwrap_or_default();
                let after = text.get(cursor + at.len()..range.end).unwrap_or_default();
                Line::from(vec![
                    Span::raw(shown(before)),
                    Span::styled(
                        if at.is_empty() {
                            " ".to_owned()
                        } else {
                            shown(at)
                        },
                        self.cursor,
                    ),
                    Span::raw(shown(after)),
                ])
            })
            .collect();
        Paragraph::new(lines).render(area, buf);
    }
}

/// `text` laid out in rows for an area `width` cells wide: the byte range
/// of each row, in order. Each line, between line breaks, starts a row,
/// and wraps between words, or between graphemes inside a word too wide
/// for a row. A row holds at most `width` less 1 cells, keeping the last
/// for the caret, though a space may stand in that cell at a wrap. A line
/// whose last row is full gets an empty row after it, where the caret
/// shows at its end.
fn rows(text: &str, width: u16) -> Vec<Range<usize>> {
    let width = usize::from(width);
    let room = width.saturating_sub(1);
    let mut rows = Vec::new();
    let mut line_start = 0;
    for line in text.split('\n') {
        let mut start = 0;
        let mut used = 0;
        // Where the row may wrap: after its last space.
        let mut wrap = None;
        for (at, grapheme) in line.grapheme_indices(true) {
            let size = cells(grapheme);
            let space = grapheme.chars().all(char::is_whitespace);
            if at > start {
                let fits = if space {
                    used + size <= width
                } else {
                    used + size <= room
                };
                if !fits {
                    let end = wrap.filter(|&wrap| !space && wrap > start).unwrap_or(at);
                    rows.push(line_start + start..line_start + end);
                    start = end;
                    used = cells(line.get(start..at).unwrap_or_default());
                    wrap = None;
                    if used + size > room && at > start {
                        rows.push(line_start + start..line_start + at);
                        start = at;
                        used = 0;
                    }
                }
            }
            used += size;
            if space {
                wrap = Some(at + grapheme.len());
            }
        }
        let end = line_start + line.len();
        rows.push(line_start + start..end);
        if used > room {
            rows.push(end..end);
        }
        line_start = end + 1;
    }
    rows
}

/// The row of `rows` the cursor at `cursor` shows in: the last that starts
/// at or before it, so a cursor where a row wraps shows at the start of the
/// next.
fn row_of(rows: &[Range<usize>], cursor: usize) -> usize {
    rows.iter()
        .rposition(|row| row.start <= cursor)
        .unwrap_or_default()
}

/// The cells `text` takes on a row.
fn cells(text: &str) -> usize {
    text.graphemes(true)
        .map(|grapheme| {
            if grapheme == "\t" {
                TAB
            } else {
                Span::raw(grapheme).width()
            }
        })
        .sum()
}

/// `text` as a row shows it: each tab as spaces.
fn shown(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB))
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let limit = grouped(LIMIT);
        match self {
            Self::PasteCut { left_out: 1 } => write!(
                f,
                "Paste cut at the {limit}-character limit: 1 character left out."
            ),
            Self::PasteCut { left_out } => write!(
                f,
                "Paste cut at the {limit}-character limit: {} characters left out.",
                grouped(*left_out)
            ),
            Self::AtLimit => write!(f, "Reasoning is at its {limit}-character limit."),
        }
    }
}

/// `number` with its thousands set apart by commas: `4,000`.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Shows the reasoning's length, never its text.
impl fmt::Debug for ReasoningEditor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReasoningEditor")
            .field("reasoning_bytes", &self.text.len())
            .field("cursor", &self.cursor)
            .field("notice", &self.notice)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;

    use super::*;
    use crate::keymap;
    use crate::theme::{ColorSupport, Theme};

    /// How wide the editor is drawn in these tests, unless a test says.
    const WIDTH: u16 = 40;

    /// Presses `key` in `editor`, drawn [`WIDTH`] cells wide.
    fn press(editor: &mut ReasoningEditor, key: KeyEvent) -> Editing {
        press_at(editor, key, WIDTH)
    }

    /// Presses `key` in `editor`, drawn `width` cells wide, as the app
    /// dispatches a key to a focused text entry: a typed character is text
    /// before any binding, and any other key goes through the editor's
    /// keymap (rule 29).
    fn press_at(editor: &mut ReasoningEditor, key: KeyEvent, width: u16) -> Editing {
        match keymap::typed(&key)
            .map(Command::Type)
            .or_else(|| keymap::command(KEYMAP, &key))
        {
            Some(command) => editor.apply(command, width),
            None => Editing::Stay,
        }
    }

    /// `editor` drawn in an area `width` by `height` cells, its caret shown.
    fn drawn(editor: &ReasoningEditor, width: u16, height: u16) -> Buffer {
        drawn_with(editor, Caret::Shown, width, height)
    }

    /// `editor` drawn in an area `width` by `height` cells, with `caret`.
    fn drawn_with(editor: &ReasoningEditor, caret: Caret, width: u16, height: u16) -> Buffer {
        let theme = Theme::new(ColorSupport::TrueColor);
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        editor.view(&theme, caret).render(buffer.area, &mut buffer);
        buffer
    }

    /// Snapshots `editor` drawn with `caret` six rows high, at both snapshot
    /// widths.
    fn assert_snapshots(name: &str, editor: &ReasoningEditor, caret: Caret) {
        for width in [120, 80] {
            insta::assert_debug_snapshot!(
                format!("{name}_{width}"),
                drawn_with(editor, caret, width, 6)
            );
        }
    }

    /// Two paragraphs of reasoning, the cursor in the first.
    fn reasoning() -> ReasoningEditor {
        let mut editor = ReasoningEditor::default();
        type_text(
            &mut editor,
            "B: the retry sends the same idempotency key as the first request, so the \
             payment provider sees one payment, however many times the request is sent. \
             A fresh key per attempt would charge again.\n\
             C caps the retries, but a cap alone doesn't stop a double charge.",
        );
        press_all(&mut editor, &[KeyCode::Up; 3]);
        editor
    }

    #[test]
    fn while_editing() {
        assert_snapshots("while_editing", &reasoning(), Caret::Shown);
    }

    #[test]
    fn not_editing() {
        assert_snapshots("not_editing", &reasoning(), Caret::Hidden);
    }

    #[test]
    fn with_a_cut_paste() {
        let mut editor = ReasoningEditor::default();
        editor.paste(&"The key dedupes. ".repeat(235));
        editor.paste("The provider sees one payment.");
        assert_snapshots("with_a_cut_paste", &editor, Caret::Shown);
    }

    /// The text of each row in `buffer`, without the blank cells after it.
    /// A wide character's symbol is in its first cell, so the cells it
    /// covers after that are skipped.
    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < buffer.area.width {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    x += u16::try_from(Span::raw(symbol).width().max(1)).expect("a few cells");
                }
                row.trim_end().to_owned()
            })
            .collect()
    }

    /// Where the caret is drawn in `buffer`: the one reversed cell.
    fn caret(buffer: &Buffer) -> Option<(u16, u16)> {
        let area = buffer.area;
        let mut reversed = (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .filter(|&at| buffer[at].modifier.contains(Modifier::REVERSED));
        let first = reversed.next();
        assert_eq!(reversed.next(), None, "one caret at most");
        first
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Types each character of `text`, pressing Enter for each line break.
    fn type_text(editor: &mut ReasoningEditor, text: &str) {
        for c in text.chars() {
            let code = if c == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(c)
            };
            assert_eq!(press(editor, key(code)), Editing::Stay, "{c:?}");
        }
    }

    #[test]
    fn every_key_but_esc_and_tab_types_and_enter_starts_a_new_line() {
        let mut editor = ReasoningEditor::default();

        type_text(&mut editor, "Keys q, ? and : type.\nSo do a, i, j and /.");

        assert_eq!(editor.text(), "Keys q, ? and : type.\nSo do a, i, j and /.");
    }

    #[test]
    fn esc_stops_editing_and_tab_moves_on_both_keeping_the_draft() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "Dedupes.");

        assert_eq!(press(&mut editor, key(KeyCode::Esc)), Editing::Stop);
        assert_eq!(press(&mut editor, key(KeyCode::Tab)), Editing::Next);
        // Shift+Tab doesn't leave: only Esc and Tab do.
        assert_eq!(press(&mut editor, key(KeyCode::BackTab)), Editing::Stay);
        assert_eq!(editor.text(), "Dedupes.");
    }

    /// Presses each of `codes` in `editor`.
    fn press_all(editor: &mut ReasoningEditor, codes: &[KeyCode]) {
        for &code in codes {
            assert_eq!(press(editor, key(code)), Editing::Stay, "{code:?}");
        }
    }

    // An e with a combining accent, a thumbs-up with a skin tone, and a
    // family joined by zero-width joiners: each is one grapheme of several
    // characters, typed a character at a time.
    const ACCENTED: &str = "e\u{301}";
    const THUMBS_UP: &str = "\u{1f44d}\u{1f3fd}";
    const FAMILY: &str = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";

    #[test]
    fn editing_and_moving_go_a_whole_grapheme_at_a_time() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, &format!("caf{ACCENTED}{THUMBS_UP}{FAMILY}"));

        press_all(&mut editor, &[KeyCode::Left, KeyCode::Left]);
        type_text(&mut editor, "|");
        assert_eq!(editor.text(), format!("caf{ACCENTED}|{THUMBS_UP}{FAMILY}"));

        press_all(&mut editor, &[KeyCode::Backspace, KeyCode::Backspace]);
        assert_eq!(editor.text(), format!("caf{THUMBS_UP}{FAMILY}"));

        press_all(&mut editor, &[KeyCode::Delete]);
        assert_eq!(editor.text(), format!("caf{FAMILY}"));

        press_all(&mut editor, &[KeyCode::Right, KeyCode::Right]);
        type_text(&mut editor, ".");
        assert_eq!(
            editor.text(),
            format!("caf{FAMILY}."),
            "Right stops at the end"
        );

        press_all(
            &mut editor,
            &[KeyCode::Left, KeyCode::Left, KeyCode::Backspace],
        );
        assert_eq!(editor.text(), format!("ca{FAMILY}."));
    }

    #[test]
    fn home_and_end_go_to_the_ends_of_the_line_and_a_line_break_erases_like_a_character() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "one\ntwo\nthree");

        press_all(&mut editor, &[KeyCode::Home]);
        type_text(&mut editor, ">");
        press_all(&mut editor, &[KeyCode::Left, KeyCode::Left]);
        type_text(&mut editor, "<");
        press_all(&mut editor, &[KeyCode::Home]);
        type_text(&mut editor, "#");
        press_all(&mut editor, &[KeyCode::End]);
        type_text(&mut editor, "$");
        assert_eq!(editor.text(), "one\n#two<$\n>three");

        press_all(&mut editor, &[KeyCode::Home, KeyCode::Backspace]);
        assert_eq!(editor.text(), "one#two<$\n>three");
        press_all(&mut editor, &[KeyCode::End, KeyCode::Delete]);
        assert_eq!(editor.text(), "one#two<$>three");

        press_all(
            &mut editor,
            &[KeyCode::Home, KeyCode::Backspace, KeyCode::Left],
        );
        press_all(
            &mut editor,
            &[KeyCode::End, KeyCode::Delete, KeyCode::Right],
        );
        type_text(&mut editor, "!");
        assert_eq!(
            editor.text(),
            "one#two<$>three!",
            "nothing to erase at the start or delete at the end"
        );
    }

    #[test]
    fn lines_wrap_between_words_to_the_width_the_editor_is_drawn_at() {
        let mut editor = ReasoningEditor::default();
        type_text(
            &mut editor,
            "The retry reuses the idempotency key, so the provider deduplicates.",
        );

        // A row keeps its last cell for the caret.
        assert_eq!(
            rows(&drawn(&editor, 20, 6)),
            [
                "The retry reuses",
                "the idempotency",
                "key, so the",
                "provider",
                "deduplicates.",
                "",
            ]
        );
        assert_eq!(caret(&drawn(&editor, 20, 6)), Some((13, 4)));
        assert_eq!(
            rows(&drawn(&editor, 80, 2)),
            [
                "The retry reuses the idempotency key, so the provider deduplicates.",
                "",
            ]
        );
    }

    #[test]
    fn a_word_too_wide_for_a_row_breaks_between_graphemes_measured_in_cells() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "abcdefghijklmnopqrstuvwxyz\n日本語の文章");

        assert_eq!(
            rows(&drawn(&editor, 10, 6)),
            ["abcdefghi", "jklmnopqr", "stuvwxyz", "日本語の", "文章", ""]
        );
        // Each of these characters is two cells wide.
        assert_eq!(caret(&drawn(&editor, 10, 6)), Some((4, 4)));
    }

    #[test]
    fn up_and_down_move_between_lines_keeping_the_column_where_they_can() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "first line\nab\nthird line");

        press_all(&mut editor, &[KeyCode::Up]);
        type_text(&mut editor, "1");
        press_all(&mut editor, &[KeyCode::Up]);
        type_text(&mut editor, "2");
        press_all(&mut editor, &[KeyCode::Down]);
        type_text(&mut editor, "3");
        press_all(&mut editor, &[KeyCode::Down]);
        type_text(&mut editor, "4");
        assert_eq!(editor.text(), "fir2st line\nab13\nthir4d line");

        press_all(&mut editor, &[KeyCode::Down]);
        type_text(&mut editor, "5");
        press_all(&mut editor, &[KeyCode::Up, KeyCode::Up, KeyCode::Up]);
        type_text(&mut editor, "6");
        assert_eq!(
            editor.text(),
            "fir26st line\nab13\nthir45d line",
            "Up and Down stop at the first and last rows"
        );
    }

    #[test]
    fn up_and_down_move_between_rows_as_they_are_drawn() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "aaaa bbbb cccc");
        let press = |editor: &mut ReasoningEditor, code| press_at(editor, key(code), 6);
        assert_eq!(rows(&drawn(&editor, 6, 3)), ["aaaa", "bbbb", "cccc"]);

        press(&mut editor, KeyCode::Up);
        type_text(&mut editor, "2");
        // The cursor never goes past a row's last character onto the next
        // row: there, it would show at the start of the row after.
        press(&mut editor, KeyCode::Up);
        type_text(&mut editor, "1");
        press(&mut editor, KeyCode::Down);
        type_text(&mut editor, "3");

        assert_eq!(editor.text(), "aaaa1 bbbb23 cccc");
    }

    #[test]
    fn the_rows_shown_follow_the_cursor() {
        let mut editor = ReasoningEditor::default();
        let lines: Vec<String> = (1..=10).map(|line| format!("line {line}")).collect();
        type_text(&mut editor, &lines.join("\n"));

        assert_eq!(
            rows(&drawn(&editor, WIDTH, 4)),
            ["line 7", "line 8", "line 9", "line 10"]
        );
        press_all(&mut editor, &[KeyCode::Up; 4]);
        assert_eq!(
            rows(&drawn(&editor, WIDTH, 4)),
            ["line 4", "line 5", "line 6", "line 7"],
            "the cursor's row stays in the middle while it can"
        );
        assert_eq!(caret(&drawn(&editor, WIDTH, 4)), Some((6, 2)));
        press_all(&mut editor, &[KeyCode::Up; 5]);
        assert_eq!(
            rows(&drawn(&editor, WIDTH, 4)),
            ["line 1", "line 2", "line 3", "line 4"]
        );
    }

    #[test]
    fn a_paste_goes_in_at_the_cursor_as_one_insertion_keeping_its_line_breaks() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "Before after");
        press_all(&mut editor, &[KeyCode::Left; 5]);

        editor.paste("one\r\ntwo\rthree\n\tfour\u{7} ");
        type_text(&mut editor, "|");

        assert_eq!(editor.text(), "Before one\ntwo\nthree\n\tfour |after");
        assert_eq!(
            rows(&drawn(&editor, WIDTH, 5)),
            ["Before one", "two", "three", "    four |after", ""],
            "a tab shows as four spaces"
        );
    }

    /// How many graphemes `editor` holds.
    fn graphemes(editor: &ReasoningEditor) -> usize {
        editor.text().graphemes(true).count()
    }

    #[test]
    fn a_paste_past_the_limit_is_cut_at_the_limit_with_a_notice_until_the_next_edit() {
        let mut editor = ReasoningEditor::default();
        editor.paste(&"x".repeat(3_997));

        editor.paste("abcdefghij");

        assert_eq!(graphemes(&editor), 4_000);
        assert!(editor.text().ends_with("xabc"));
        let notice = "Paste cut at the 4,000-character limit: 7 characters left out.";
        assert_eq!(
            rows(&drawn(&editor, 80, 4)).last().map(String::as_str),
            Some(notice)
        );
        press_all(&mut editor, &[KeyCode::Left, KeyCode::Up, KeyCode::End]);
        assert_eq!(
            rows(&drawn(&editor, 80, 4)).last().map(String::as_str),
            Some(notice),
            "moving the cursor is no edit"
        );
        press_all(&mut editor, &[KeyCode::Backspace]);
        assert!(
            !rows(&drawn(&editor, 80, 4))
                .iter()
                .any(|row| row.contains("limit")),
            "the next edit clears the notice"
        );
    }

    #[test]
    fn the_limit_counts_graphemes_not_characters() {
        let mut editor = ReasoningEditor::default();
        editor.paste(&"x".repeat(3_998));

        editor.paste(&FAMILY.repeat(3));

        assert_eq!(
            editor.text(),
            format!("{}{}", "x".repeat(3_998), FAMILY.repeat(2))
        );
        assert_eq!(
            rows(&drawn(&editor, 80, 4)).last().map(String::as_str),
            Some("Paste cut at the 4,000-character limit: 1 character left out.")
        );
    }

    #[test]
    fn typing_at_the_limit_inserts_nothing() {
        let mut editor = ReasoningEditor::default();
        editor.paste(&"x".repeat(4_000));
        press_all(&mut editor, &[KeyCode::Home]);

        type_text(&mut editor, "y\n");

        assert_eq!(editor.text(), "x".repeat(4_000));
        assert_eq!(
            rows(&drawn(&editor, 80, 4)).last().map(String::as_str),
            Some("Reasoning is at its 4,000-character limit.")
        );
        press_all(&mut editor, &[KeyCode::Delete]);
        type_text(&mut editor, "y");
        assert_eq!(editor.text(), format!("y{}", "x".repeat(3_999)));
    }

    #[test]
    fn a_notice_wraps_below_the_text_and_the_text_keeps_the_rows_above() {
        let mut editor = ReasoningEditor::default();
        editor.paste(&"x\n".repeat(1_999));
        editor.paste("abc");

        assert_eq!(
            rows(&drawn(&editor, 30, 5)),
            [
                "x",
                "ab",
                "Paste cut at the",
                "4,000-character limit: 1",
                "character left out.",
            ]
        );
        assert_eq!(caret(&drawn(&editor, 30, 5)), Some((2, 1)));
    }

    #[test]
    fn whatever_the_keys_the_cursor_stays_between_graphemes() {
        let texts = [
            format!("caf{ACCENTED} {THUMBS_UP}\n{FAMILY}{FAMILY} x"),
            "\u{1f1fa}\u{1f1f8}\u{1f1eb}\u{1f1f7}\t日本語の文章\n\n  spaced   out  ".to_owned(),
        ];
        let commands = [
            Command::Left,
            Command::Right,
            Command::Up,
            Command::Down,
            Command::LineStart,
            Command::LineEnd,
            Command::Erase,
            Command::Delete,
        ];
        for text in &texts {
            for width in 0..=7 {
                for first in commands {
                    for second in commands {
                        let mut editor = ReasoningEditor::default();
                        editor.paste(text);
                        editor.apply(Command::LineStart, width);
                        editor.apply(first, width);
                        editor.apply(second, width);
                        drawn(&editor, width, 3);
                        let before = editor.text().to_owned();

                        editor.apply(Command::Type('|'), width);

                        let at = editor.text().find('|').expect("typed");
                        let between = before
                            .grapheme_indices(true)
                            .map(|(at, _)| at)
                            .chain([before.len()])
                            .any(|boundary| boundary == at);
                        assert!(
                            between,
                            "{first:?} then {second:?} at {width} left the cursor inside a grapheme"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn debug_output_never_shows_the_reasoning() {
        let mut editor = ReasoningEditor::default();
        type_text(&mut editor, "The key dedupes.");

        let debug = format!("{editor:?}");

        assert!(!debug.contains("dedupes"), "{debug}");
    }
}
