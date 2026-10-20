//! The connected code limit and the room under the size limit in the
//! outbound review: connected code is kept within both, the most relevant
//! first, and the review says how much was not sent and why.

use crate::connected_code::{REVIEW_NEWEST, connected_code};
use crate::repo::TestRepo;
use crate::session::{NO_COLOR, Session, Setup, columns};

/// `ops.py` before the newest commit changes its `sub`.
const OPS: &str = "def sub(a, b):\n    return a - b\n";

/// A Python module whose `sub` is called once from each of `callers`, a
/// path and the bytes its caller sends, with a newest commit that changes
/// `sub`.
fn subtracting(callers: &[(&str, usize)]) -> TestRepo {
    let repo = TestRepo::new("ops");
    repo.write("ops.py", OPS);
    for &(path, size) in callers {
        repo.write(path, caller(path, size));
    }
    repo.commit("Add the callers");
    repo.write("ops.py", OPS.replace("a - b", "b - a"));
    repo.commit("Subtract the other way round");
    repo
}

/// A file at `path` whose one caller of `sub` sends `size` bytes, its path
/// and its lines, made up with a comment.
fn caller(path: &str, size: usize) -> String {
    let head = "def caller(a):\n";
    let tail = "    return sub(a, a)\n";
    let comment = size - path.len() - head.len() - tail.len() - "    # \n".len();
    format!(
        "from ops import sub\n\n\n{head}    # {}\n{tail}",
        "x".repeat(comment)
    )
}

/// Five callers of `sub` that send 600 bytes each.
const FIVE_CALLERS: &[(&str, usize)] = &[
    ("use_a.py", 600),
    ("use_b.py", 600),
    ("use_c.py", 600),
    ("use_d.py", 600),
    ("use_e.py", 600),
];

/// The review of `repo`'s newest commit once its connected code is found,
/// `config` being the learner's config file.
fn reviewed(repo: &TestRepo, config: Option<&str>) -> Session {
    let mut session = Session::start(
        Setup {
            config,
            variables: NO_COLOR,
            start: Some(repo.root()),
            ..Setup::default()
        },
        120,
        60,
    );
    session.press(REVIEW_NEWEST);
    session.settle();
    session
}

#[test]
fn callers_beyond_the_learners_limit_are_not_sent_and_the_review_says_how_many() {
    let repo = subtracting(FIVE_CALLERS);

    let session = reviewed(&repo, Some("connected_code_limit_kb = 2\n"));

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 3 ranges · 2 KB of 2 KB limit",
            "use_a.py  lines 4–6  1 KB  included  calls `sub`",
            "use_b.py  lines 4–6  1 KB  included  calls `sub`",
            "use_c.py  lines 4–6  1 KB  included  calls `sub`",
            "2 more not sent: over the connected code limit",
        ]
    );
}

#[test]
fn leaving_callers_out_frees_their_room_without_sending_any_that_were_not_sent() {
    let repo = subtracting(FIVE_CALLERS);
    let mut session = reviewed(&repo, Some("connected_code_limit_kb = 2\n"));

    // The change, the touched file, then the first two callers.
    session.press("j j <Space> j <Space>");

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 1 range · 1 KB of 2 KB limit",
            "use_a.py  lines 4–6  1 KB  left out by you  calls `sub`",
            "> use_b.py  lines 4–6  1 KB  left out by you  calls `sub`",
            "use_c.py  lines 4–6  1 KB  included         calls `sub`",
            "2 more not sent: over the connected code limit",
        ]
    );
}

/// The review of `repo`'s newest commit once its connected code is found,
/// with `learner` the learner's config file and `repository` the
/// repository's.
fn reviewed_with(repo: &TestRepo, learner: Option<&str>, repository: &str) -> Session {
    repo.write(".whetstone/config.toml", repository);
    reviewed(repo, learner)
}

#[test]
fn a_repository_can_lower_the_limit_and_the_review_says_so() {
    let repo = subtracting(FIVE_CALLERS);

    let session = reviewed_with(&repo, None, "connected_code_limit_kb = 1\n");

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 1 range · 1 KB of 1 KB limit · limit lowered by this repository",
            "use_a.py  lines 4–6  1 KB  included  calls `sub`",
            "4 more not sent: over the connected code limit",
        ]
    );
}

#[test]
fn a_repository_cannot_raise_the_learners_limit() {
    let repo = subtracting(FIVE_CALLERS);

    let session = reviewed_with(
        &repo,
        Some("connected_code_limit_kb = 2\n"),
        "connected_code_limit_kb = 400\n",
    );

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 3 ranges · 2 KB of 2 KB limit",
            "use_a.py  lines 4–6  1 KB  included  calls `sub`",
            "use_b.py  lines 4–6  1 KB  included  calls `sub`",
            "use_c.py  lines 4–6  1 KB  included  calls `sub`",
            "2 more not sent: over the connected code limit",
        ]
    );
}

#[test]
fn without_a_limit_set_connected_code_is_kept_within_100_kb() {
    let repo = subtracting(&[("use_a.py", 60_000), ("use_b.py", 60_000)]);

    let session = reviewed(&repo, None);

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 1 range · 60 KB of 100 KB limit",
            "use_a.py  lines 4–6  60 KB  included  calls `sub`",
            "1 more not sent: over the connected code limit",
        ]
    );
}

#[test]
fn a_limit_of_0_sends_no_connected_code() {
    let repo = subtracting(FIVE_CALLERS);

    let session = reviewed(&repo, Some("connected_code_limit_kb = 0\n"));

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: none sent · 0 KB of 0 KB limit",
            "5 more not sent: over the connected code limit",
        ]
    );
    assert_eq!(
        columns(&session.section("Files")),
        [["ops.py", "1 KB", "included"]]
    );
}

#[test]
fn a_repository_can_lower_the_limit_to_0() {
    let repo = subtracting(FIVE_CALLERS);

    let session = reviewed_with(
        &repo,
        Some("connected_code_limit_kb = 2\n"),
        "exclude = []\nconnected_code_limit_kb = 0\n",
    );

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: none sent · 0 KB of 0 KB limit · limit lowered by this repository",
            "5 more not sent: over the connected code limit",
        ]
    );
}

/// From home: open the repository's picker, choose the newest commit, and
/// open its outbound review, with the repository's one area selected.
const REVIEW_NEWEST_WITH_ONE_AREA: &str = "<Enter> j <Space> k <S-Tab> <Enter>";

/// [`subtracting`] with one area of the repository's own, so the areas'
/// size is small and fixed, and a newest commit that also changes the
/// first line of `notes.txt`, which is as long as it must be for the change
/// to leave `room` bytes under the 400 KB size limit.
fn subtracting_beside_notes(room: usize, callers: &[(&str, usize)]) -> TestRepo {
    // The commit message, both touched files' paths and diffs, `ops.py`
    // and the area.
    const REST: usize = 679;
    let notes = 400_000 - REST - room;
    let line = format!("{}\n", "x".repeat(99));
    let mut text = "Notes\n".to_owned();
    while text.len() + line.len() <= notes {
        text.push_str(&line);
    }
    text.push_str(&"x".repeat(notes - text.len() - 1));
    text.push('\n');
    let repo = TestRepo::new("ops");
    repo.write(
        ".whetstone/areas/ops.md",
        "+++\nname = \"Ops\"\ndescription = \"How it adds up\"\n+++\n\nLook at the ops.\n",
    )
    .write("ops.py", OPS)
    .write("notes.txt", &text);
    for &(path, size) in callers {
        repo.write(path, caller(path, size));
    }
    repo.commit("Add the callers");
    repo.write("ops.py", OPS.replace("a - b", "b - a"))
        .write("notes.txt", text.replacen("Notes", "Notes!", 1));
    repo.commit("Subtract the other way round");
    repo
}

/// The review of `repo`'s newest commit, opened with its one area in a
/// `width` by `height` terminal, before its connected code is found.
fn opened_with_one_area(repo: &TestRepo, width: u16, height: u16) -> Session {
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            start: Some(repo.root()),
            ..Setup::default()
        },
        width,
        height,
    );
    session.press(REVIEW_NEWEST_WITH_ONE_AREA);
    session
}

#[test]
fn with_the_change_near_400_kb_no_caller_has_room() {
    let repo = subtracting_beside_notes(1_250, &[("use_a.py", 2_000), ("use_b.py", 3_000)]);
    let mut session = opened_with_one_area(&repo, 120, 60);
    assert_eq!(session.section("Size"), ["399 KB of 400 KB"]);

    session.settle();

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: none sent · 0 KB of 100 KB limit",
            "2 more not sent: no room under the size limit",
        ]
    );
    assert_eq!(session.section("Size"), ["399 KB of 400 KB"]);
}

/// Three callers in path order: the first too large for 3.5 KB of room,
/// the second small enough, and the third too large for what it leaves.
const LARGE_SMALL_LARGE: &[(&str, usize)] = &[
    ("use_a.py", 5_000),
    ("use_b.py", 1_000),
    ("use_c.py", 4_000),
];

#[test]
fn a_caller_with_no_room_is_skipped_whole_while_a_smaller_one_after_it_is_sent() {
    let repo = subtracting_beside_notes(3_500, LARGE_SMALL_LARGE);
    let mut session = opened_with_one_area(&repo, 120, 60);
    assert_eq!(session.section("Size"), ["397 KB of 400 KB"]);

    session.settle();

    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 1 range · 1 KB of 100 KB limit",
            "use_b.py  lines 4–6  1 KB  included  calls `sub`",
            "2 more not sent: no room under the size limit",
        ]
    );
    assert_eq!(session.section("Size"), ["398 KB of 400 KB"]);
}

#[test]
fn leaving_the_large_touched_file_out_sends_none_of_the_callers_that_had_no_room() {
    let repo = subtracting_beside_notes(3_500, LARGE_SMALL_LARGE);
    let mut session = opened_with_one_area(&repo, 120, 60);
    session.settle();

    // The change, `notes.txt`, then `ops.py`.
    session.press("j <Space>");

    assert_eq!(
        session.row_with("notes.txt"),
        "> notes.txt  397 KB  left out by you"
    );
    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 1 range · 1 KB of 100 KB limit",
            "use_b.py  lines 4–6  1 KB  included  calls `sub`",
            "2 more not sent: no room under the size limit",
        ]
    );
    assert_eq!(session.section("Size"), ["2 KB of 400 KB"]);
}

/// A review whose repository lowers the limit to 1 KB with 3.5 KB of room
/// left: the first caller is sent, the second is over the limit, and the
/// third has no room. Focus is on the one caller sent.
fn over_the_limit_and_with_no_room(width: u16, height: u16) -> Session {
    let repo = subtracting_beside_notes(
        3_500,
        &[("use_a.py", 600), ("use_b.py", 600), ("use_c.py", 5_000)],
    );
    repo.write(".whetstone/config.toml", "connected_code_limit_kb = 1\n");
    let mut session = opened_with_one_area(&repo, width, height);
    session.settle();
    // The change, `notes.txt`, `ops.py`, then the caller.
    session.press("j j j");
    session
}

#[test]
fn connected_code_over_the_limit_and_with_no_room_at_120x32() {
    let session = over_the_limit_and_with_no_room(120, 32);

    insta::assert_debug_snapshot!(
        "connected_code_over_the_limit_and_with_no_room_120x32",
        session.frame()
    );
}

#[test]
fn connected_code_over_the_limit_and_with_no_room_at_80x24() {
    let session = over_the_limit_and_with_no_room(80, 24);

    insta::assert_debug_snapshot!(
        "connected_code_over_the_limit_and_with_no_room_80x24",
        session.frame()
    );
}
