//! The search box with wide characters and emoji: each character takes its
//! width in cells, Backspace erases a whole one, and a long search keeps its
//! end in view. `viewer.rs` covers a letter with a combining accent.

use ratatui::style::Modifier;

use crate::session::Session;
use crate::viewer::typing;

/// The keys from home that open the first curated change in the viewer:
/// open the picker, move past its five areas onto the change, then press
/// Enter.
const OPEN_A_CHANGE: &str = "<Enter> j j j j j j <Enter>";

/// The search box as drawn: each character with the cell it starts at,
/// counted from the cell after the `/` prompt, then the cell the cursor is
/// in. Blank cells, such as the one a wide character covers, are left out.
fn search_box(session: &Session) -> (Vec<(u16, String)>, u16) {
    let frame = session.frame();
    let y = frame.area.height - 3;
    let cells: Vec<_> = (0..frame.area.width).map(|x| &frame[(x, y)]).collect();
    let prompt = cells
        .iter()
        .position(|cell| cell.symbol() == "/")
        .expect("the search box is open");
    let after_prompt = |x: usize| u16::try_from(x - prompt - 1).expect("rows are narrow");
    let cursor = cells
        .iter()
        .position(|cell| cell.modifier.contains(Modifier::REVERSED))
        .expect("the search box shows its cursor");
    let characters = (prompt + 1..cursor)
        .filter(|&x| cells[x].symbol() != " ")
        .map(|x| (after_prompt(x), cells[x].symbol().to_owned()))
        .collect();
    (characters, after_prompt(cursor))
}

/// `characters` paired with the cell each starts at.
fn at(characters: &[(u16, &str)]) -> Vec<(u16, String)> {
    characters
        .iter()
        .map(|&(x, character)| (x, character.to_owned()))
        .collect()
}

#[test]
fn cjk_characters_and_emoji_in_the_search_box_take_their_width_in_cells() {
    let mut session = Session::plain(80, 24);
    session.press(OPEN_A_CHANGE);

    session.press(&format!("/ {}", typing("日本語👍a")));

    // Each CJK character and emoji is two cells wide, the letter one.
    assert_eq!(
        search_box(&session),
        (
            at(&[(0, "日"), (2, "本"), (4, "語"), (6, "👍"), (8, "a")]),
            9
        ),
        "{}",
        session.text()
    );
}

#[test]
fn backspace_erases_a_whole_emoji_even_one_typed_as_several_code_points() {
    let mut session = Session::plain(80, 24);
    session.press(OPEN_A_CHANGE);

    // A woman, a zero width joiner and a laptop make one emoji: a woman
    // technologist. A thumbs up and a skin tone make another.
    session.press(&format!(
        "/ {}",
        typing("a\u{1f469}\u{200d}\u{1f4bb}\u{1f44d}\u{1f3fd}")
    ));
    assert_eq!(
        search_box(&session),
        (
            at(&[
                (0, "a"),
                (1, "\u{1f469}\u{200d}\u{1f4bb}"),
                (3, "\u{1f44d}\u{1f3fd}")
            ]),
            5
        ),
        "{}",
        session.text()
    );

    session.press("<BS>");
    assert_eq!(
        search_box(&session),
        (at(&[(0, "a"), (1, "\u{1f469}\u{200d}\u{1f4bb}")]), 3),
        "{}",
        session.text()
    );

    session.press("<BS>");
    assert_eq!(
        search_box(&session),
        (at(&[(0, "a")]), 1),
        "{}",
        session.text()
    );
}

#[test]
fn a_long_search_with_wide_characters_keeps_its_end_in_view() {
    let mut session = Session::plain(50, 12);
    session.press(OPEN_A_CHANGE);

    // Eighty-three cells of text, more than the row holds.
    session.press(&format!("/ {}", typing(&format!("{}end", "日".repeat(40)))));

    // The row is 46 cells wide, inside the screen's margins. The prompt and
    // the cursor take a cell each and "end" takes three, which leaves room
    // for twenty two-cell characters, and a cell to spare.
    let mut expected: Vec<(u16, &str)> = (0..20).map(|index| (index * 2, "日")).collect();
    expected.extend([(40, "e"), (41, "n"), (42, "d")]);
    assert_eq!(
        search_box(&session),
        (at(&expected), 43),
        "whole wide characters, then the end of the search:\n{}",
        session.text()
    );
}
