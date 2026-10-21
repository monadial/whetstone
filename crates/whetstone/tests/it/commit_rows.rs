//! The commit list's rows: each commit's choice marker, short ID, author
//! date, author, summary and size, in aligned columns. The date is worded
//! from the app's clock, the size is found in the background for the rows
//! on screen, and narrow terminals drop the size, then the author, and
//! shorten the date, before the summary is squeezed.

use ratatui::style::{Color, Modifier};

use crate::colors::colors_used;
use crate::commit_paging::{CHANGES, TO_THE_NEWEST, history_of};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, Setup};
use crate::viewer::typing;

/// When the session's clock says it is: 2026-10-21 10:15:02 UTC.
const NOW: i64 = 1_792_577_702;

/// A repository called `payments` whose four commits are by three
/// authors, at dates worded each way, newest first:
///
/// - "Bump the counter to 1", by Ada Learner just now, touching one file;
/// - "Retry a timed-out charge with its original key", by Tomas
///   Mihalicka two hours ago, two hours east of UTC, touching three;
/// - "Cache payments per tenant", by Jana Kovacova on 14 October, a week
///   ago, touching two;
/// - "Start the payments service", the first commit, by Ada Learner on 14
///   October 2025.
fn payments() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    // 2025-10-14 09:00 UTC.
    repo.commit_by(
        "Start the payments service",
        "Ada Learner",
        "@1760432400 +0000",
    );
    repo.write("README.md", "# Payments\n\nCached per tenant.\n");
    repo.write("src/cache.rs", "pub struct Cache;\n\nimpl Cache {}\n");
    // 2026-10-14 09:00 UTC.
    repo.commit_by(
        "Cache payments per tenant",
        "Jana Kovacova",
        "@1791968400 +0000",
    );
    repo.write("src/cache.rs", "pub struct Cache;\n\nimpl Cache { }\n");
    repo.write("src/retry.rs", "pub fn retry() {\n    charge();\n}\n\n");
    repo.write("tests/retry.rs", "#[test]\nfn retries() {}\n");
    repo.commit_by(
        "Retry a timed-out charge with its original key",
        "Tomas Mihalicka",
        &format!("@{} +0200", NOW - 2 * 3_600),
    );
    repo.write("counter.txt", "1\n");
    repo.commit_by(
        "Bump the counter to 1",
        "Ada Learner",
        &format!("@{} +0000", NOW - 30),
    );
    repo
}

/// The short IDs of `repo`'s commits, newest first.
fn short_ids(repo: &TestRepo) -> Vec<String> {
    repo.git(&["log", "--format=%h"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The commit rows in view, trimmed, without the focus marker.
fn in_view(session: &Session) -> Vec<String> {
    session
        .section(CHANGES)
        .into_iter()
        .filter(|row| row.contains("( ) ") || row.contains("(x) "))
        .map(|row| row.trim_start_matches("> ").to_owned())
        .collect()
}

/// A row's cells: its text between runs of two spaces or more, each with
/// the column it starts in.
fn cells(row: &str) -> Vec<(usize, String)> {
    let mut cells = Vec::new();
    let mut start = None;
    let mut spaces = 0;
    let chars: Vec<char> = row.chars().collect();
    for (column, &cell) in chars.iter().enumerate() {
        if cell == ' ' {
            spaces += 1;
            continue;
        }
        if spaces >= 2 || start.is_none() {
            if let Some(start) = start {
                cells.push((start, chars[start..column - spaces].iter().collect()));
            }
            start = Some(column);
        }
        spaces = 0;
    }
    if let Some(start) = start {
        cells.push((start, chars[start..chars.len() - spaces].iter().collect()));
    }
    cells
}

/// The text of each commit row in view, cell by cell.
fn texts(session: &Session) -> Vec<Vec<String>> {
    in_view(session)
        .iter()
        .map(|row| cells(row).into_iter().map(|(_, text)| text).collect())
        .collect()
}

/// The column each cell after the marker and ID starts in, in each commit
/// row in view.
fn starts(session: &Session) -> Vec<Vec<usize>> {
    in_view(session)
        .iter()
        .map(|row| cells(row).into_iter().skip(1).map(|(at, _)| at).collect())
        .collect()
}

/// `cells` as owned strings, for comparing with [`texts`].
fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|&cell| cell.to_owned()).collect()
}

#[test]
fn each_row_shows_the_id_date_author_summary_and_size_in_aligned_columns() {
    let repo = payments();
    let ids = short_ids(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    session.settle();

    assert_eq!(
        texts(&session),
        [
            row(&[
                &format!("( ) {}", ids[0]),
                "just now",
                "Ada Learner",
                "Bump the counter to 1",
                "1 file",
                "+1 −0",
            ]),
            row(&[
                &format!("( ) {}", ids[1]),
                "2 hours ago",
                "Tomas M.",
                "Retry a timed-out charge with its original key",
                "3 files",
                "+7 −1",
            ]),
            row(&[
                &format!("( ) {}", ids[2]),
                "Oct 14",
                "Jana K.",
                "Cache payments per tenant",
                "2 files",
                "+5 −0",
            ]),
            row(&[
                &format!("( ) {}", ids[3]),
                "Oct 14, 2025",
                "Ada Learner",
                "Start the payments service",
                "first commit",
            ]),
        ],
        "{}",
        session.text()
    );
    // The date, author, summary and size each start in one column, and the
    // lines added and removed in one column inside the size's. The first
    // commit's label stands in the size's column.
    let starts = starts(&session);
    for row in &starts[1..3] {
        assert_eq!(row, &starts[0], "{}", session.text());
    }
    assert_eq!(starts[3][..4], starts[0][..4], "{}", session.text());
    // The summary takes the remaining width, so the size ends the row.
    let last = in_view(&session)[1].chars().count();
    assert_eq!(last, 120 - 2 * 2 - 2, "{}", session.text());
}

#[test]
fn the_size_shows_an_ellipsis_until_it_is_found_in_the_background() {
    let repo = payments();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");

    assert_eq!(
        ends(&session),
        ["…", "…", "…", "first commit"],
        "{}",
        session.text()
    );

    session.settle();
    assert_eq!(
        texts(&session)[0][4..],
        ["1 file", "+1 −0"],
        "{}",
        session.text()
    );
}

/// The last cell of each commit row in view: the end of its size, or what
/// stands in for one.
fn ends(session: &Session) -> Vec<String> {
    texts(session)
        .iter()
        .map(|cells| cells.last().cloned().unwrap_or_default())
        .collect()
}

#[test]
fn sizes_are_found_only_for_the_rows_on_screen_and_kept_once_found() {
    let repo = history_of(250);
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);
    session.settle();
    let first_page = ends(&session);
    assert_eq!(first_page, vec!["+1 −1"; 18], "{}", session.text());

    // The next page's sizes weren't found while it was off screen.
    session.press("<PageDown>");
    assert_eq!(ends(&session), vec!["…"; 18], "{}", session.text());

    // The first page's are kept, so they show at once.
    session.press("<PageUp>");
    assert_eq!(ends(&session), first_page, "{}", session.text());
}

#[test]
fn a_size_found_for_a_commit_the_filter_now_hides_is_ignored() {
    let repo = payments();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    // Every row's size is being found when the filter hides all but one.
    session.press(&typing("/kovacova"));
    session.settle();

    session.press("<Esc>");
    assert_eq!(
        ends(&session),
        ["…", "…", "+5 −0", "first commit"],
        "{}",
        session.text()
    );
    session.settle();
    assert_eq!(
        ends(&session),
        ["+1 −0", "+7 −1", "+5 −0", "first commit"],
        "{}",
        session.text()
    );
}

#[test]
fn the_columns_stay_where_they_are_as_the_sizes_are_found() {
    // No label in view holds the size's column open.
    let repo = history_of(30);
    // Where the date, author, summary and size start, if shown.
    let columns = |session: &Session| -> Vec<Vec<usize>> {
        starts(session)
            .into_iter()
            .map(|row| row.into_iter().take(4).collect())
            .collect()
    };
    for (width, height) in [(120, 32), (80, 24)] {
        let mut session = started_in(repo.root(), width, height);
        session.press("<Enter>");
        let finding = columns(&session);
        session.settle();

        assert_eq!(columns(&session), finding, "{}", session.text());
    }
}

#[test]
fn at_80_columns_the_date_shortens_and_the_size_goes() {
    let repo = payments();
    let ids = short_ids(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter>");
    // The columns are the same while the sizes are found as once they are.
    let finding = texts(&session);
    session.settle();
    assert_eq!(texts(&session), finding, "{}", session.text());

    // The first commit keeps its label after its summary.
    assert_eq!(
        texts(&session),
        [
            row(&[
                &format!("( ) {}", ids[0]),
                "now",
                "Ada Learner",
                "Bump the counter to 1",
            ]),
            row(&[
                &format!("( ) {}", ids[1]),
                "2h",
                "Tomas M.",
                "Retry a timed-out charge with its ori…",
            ]),
            row(&[
                &format!("( ) {}", ids[2]),
                "Oct 14",
                "Jana K.",
                "Cache payments per tenant",
            ]),
            row(&[
                &format!("( ) {}", ids[3]),
                "Oct 2025",
                "Ada Learner",
                "Start the payments serv…",
                "first commit",
            ]),
        ],
        "{}",
        session.text()
    );
}

#[test]
fn narrower_terminals_shorten_the_date_then_drop_the_size_then_the_author() {
    let repo = payments();
    // How many cells the newest commit's row has at each width: the
    // marker and ID, the date, the author, the summary, and the size in
    // two.
    for (width, dates, cells) in [
        (120, "just now", 6),
        (100, "just now", 6),
        (90, "now", 6),
        (80, "now", 4),
        (60, "now", 3),
    ] {
        let mut session = started_in(repo.root(), width, 24);
        session.press("<Enter>");
        session.settle();

        let newest = &texts(&session)[0];
        assert_eq!(newest[1], dates, "at {width}:\n{}", session.text());
        assert_eq!(newest.len(), cells, "at {width}:\n{}", session.text());
        assert_eq!(
            newest.last().map(String::as_str),
            Some(if cells == 6 {
                "+1 −0"
            } else {
                "Bump the counter to 1"
            }),
            "at {width}:\n{}",
            session.text()
        );
    }
}

#[test]
fn a_merge_commit_shows_its_label_in_place_of_a_size() {
    let repo = payments();
    repo.git(&["switch", "--quiet", "--create", "refunds", "HEAD~1"]);
    repo.write("src/refund.rs", "pub fn refund() {}\n");
    repo.commit("Add refunds");
    repo.git(&["switch", "--quiet", "main"]);
    repo.merge("refunds", "Merge refunds");
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    session.settle();

    let merge = &texts(&session)[0];
    assert_eq!(merge[3], "Merge refunds", "{}", session.text());
    assert_eq!(
        merge[4..],
        ["merge · vs first parent"],
        "{}",
        session.text()
    );
}

#[test]
fn the_filter_matches_an_authors_name() {
    let repo = payments();
    let ids = short_ids(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");

    session.press(&typing("/kovacova"));
    session.settle();

    let listed: Vec<String> = texts(&session).iter().map(|row| row[0].clone()).collect();
    assert_eq!(listed, [format!("( ) {}", ids[2])], "{}", session.text());

    session.press("<Esc>");
    session.press(&typing("/Ada"));
    let listed: Vec<String> = texts(&session).iter().map(|row| row[0].clone()).collect();
    assert_eq!(
        listed,
        [format!("( ) {}", ids[0]), format!("( ) {}", ids[3])],
        "{}",
        session.text()
    );
}

#[test]
fn the_id_date_author_and_size_are_muted_and_the_summary_reads_as_text() {
    let repo = payments();
    let mut session = Session::start(
        Setup {
            config: Some("colours = \"256\"\n"),
            start: Some(repo.root()),
            ..Setup::default()
        },
        120,
        32,
    );
    session.press(TO_THE_NEWEST);
    session.settle();
    let frame = session.frame();
    let rows = session.rows();
    let style_of = |y: usize, text: &str| {
        let x = rows[y]
            .chars()
            .collect::<Vec<_>>()
            .windows(text.chars().count())
            .position(|cells| cells.iter().collect::<String>() == text);
        assert!(x.is_some(), "no {text:?} in row {y}:\n{}", session.text());
        let x = u16::try_from(x.unwrap_or_default()).expect("a column fits");
        let y = u16::try_from(y).expect("a row fits");
        let cell = &frame[(x, y)];
        (cell.fg, cell.modifier)
    };
    let row_of = |text: &str| {
        rows.iter()
            .position(|row| row.contains(text))
            .unwrap_or_default()
    };
    let muted = (Color::Indexed(250), Modifier::empty());
    let text = (Color::Indexed(255), Modifier::empty());
    let focus = (Color::Indexed(111), Modifier::BOLD);

    let retry = row_of("Retry a timed-out");
    for column in ["2 hours ago", "Tomas M.", "3 files", "+7 −1"] {
        assert_eq!(style_of(retry, column), muted, "{column}");
    }
    assert_eq!(style_of(retry, "Retry a timed-out"), text);
    // The focused row keeps the focus style throughout.
    let newest = row_of("Bump the counter");
    for column in ["just now", "Ada Learner", "Bump the counter", "1 file"] {
        assert_eq!(style_of(newest, column), focus, "{column}");
    }
}

#[test]
fn without_colour_the_columns_and_markers_still_carry_the_rows() {
    let repo = payments();
    let mut coloured = Session::start(
        Setup {
            config: Some("colours = \"256\"\n"),
            start: Some(repo.root()),
            ..Setup::default()
        },
        120,
        32,
    );
    let mut plain = started_in(repo.root(), 120, 32);
    for session in [&mut coloured, &mut plain] {
        session.press(TO_THE_NEWEST);
        session.settle();
    }

    assert_eq!(
        plain.section(CHANGES),
        coloured.section(CHANGES),
        "{}",
        plain.text()
    );
    assert_eq!(
        colors_used(&plain.frame()),
        [Color::Reset].into(),
        "{}",
        plain.text()
    );
}

#[test]
fn commit_rows_120x32() {
    let repo = payments();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);
    session.settle();

    insta::assert_debug_snapshot!("commit_rows_120x32", session.frame());
}

#[test]
fn commit_rows_80x24() {
    let repo = payments();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(TO_THE_NEWEST);
    session.settle();

    insta::assert_debug_snapshot!("commit_rows_80x24", session.frame());
}
