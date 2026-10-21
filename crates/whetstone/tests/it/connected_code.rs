//! Connected code in the outbound review: the callers and callees of the
//! callables a change adds or modifies, found in the background once the
//! review opens, and listed, read and left out like touched files.

use crate::repo::TestRepo;
use crate::repository::{started_in, started_with_a_key_in};
use crate::session::{Session, Setup, columns};

/// From home: open the repository's picker, choose the newest commit, and
/// open its outbound review, with the five default areas selected.
pub(crate) const REVIEW_NEWEST: &str = "<Enter> j j j j j <Space> k k k k k <S-Tab> <Enter>";

/// The Connected code section: its summary, then its rows, trimmed, up to
/// the next blank row.
pub(crate) fn connected_code(session: &Session) -> Vec<String> {
    let rows = session.rows();
    let start = rows
        .iter()
        .position(|row| row.trim().starts_with("Connected code"));
    assert!(start.is_some(), "no Connected code:\n{}", session.text());
    rows.iter()
        .skip(start.unwrap_or_default())
        .map(|row| row.trim().to_owned())
        .take_while(|row| !row.is_empty())
        .collect()
}

/// The Connected code section's rows after its summary, split into their
/// columns, without the focus marker.
pub(crate) fn connected_rows(session: &Session) -> Vec<Vec<String>> {
    columns(&connected_code(session)[1..])
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|column| column.trim_start_matches("> ").to_owned())
                .collect()
        })
        .collect()
}

/// A Rust crate called `shop` in which `Payments::charge` validates with
/// `check::validate` and is called by `checkout`.
pub(crate) fn rust_payments() -> TestRepo {
    let repo = TestRepo::new("shop");
    repo.write("src/lib.rs", "mod check;\nmod pay;\nmod shop;\n")
        .write(
            "src/check.rs",
            "pub fn validate(amount: u32) -> bool {\n    amount > 0\n}\n",
        )
        .write("src/pay.rs", rust_pay("validate(amount)"))
        .write(
            "src/shop.rs",
            "use crate::pay::Payments;\n\
             \n\
             pub fn checkout(payments: &Payments) -> bool {\n\
             \x20   payments.charge(10)\n\
             }\n",
        );
    repo.commit("Start the shop");
    repo
}

/// `src/pay.rs`, whose `Payments::charge` returns `body`.
pub(crate) fn rust_pay(body: &str) -> String {
    format!(
        "use crate::check::validate;\n\
         \n\
         pub struct Payments;\n\
         \n\
         impl Payments {{\n\
         \x20   pub fn charge(&self, amount: u32) -> bool {{\n\
         \x20       {body}\n\
         \x20   }}\n\
         }}\n"
    )
}

#[test]
fn a_rust_change_lists_the_callers_and_callees_of_the_method_it_modifies_once_found() {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    repo.commit("Double the charge");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(connected_code(&session), ["Connected code: finding…"]);

    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB of 100 KB limit"
    );
    assert_eq!(
        connected_rows(&session),
        [
            [
                "src/check.rs",
                "lines 1–3",
                "1 KB",
                "included",
                "called by `charge`"
            ],
            [
                "src/shop.rs",
                "lines 3–5",
                "1 KB",
                "included",
                "calls `charge`"
            ],
        ]
    );
}

#[test]
fn a_python_change_lists_the_callers_and_callees_of_the_method_it_modifies() {
    let repo = TestRepo::new("shop");
    let pay = |body: &str| {
        format!(
            "from shop.check import validate\n\
             \n\
             \n\
             class Payments:\n\
             \x20   def charge(self, amount):\n\
             \x20       return {body}\n"
        )
    };
    repo.write("shop/__init__.py", "")
        .write(
            "shop/check.py",
            "def validate(amount):\n    return amount > 0\n",
        )
        .write("shop/pay.py", pay("validate(amount)"))
        .write(
            "shop/cart.py",
            "from shop.pay import Payments\n\
             \n\
             \n\
             def checkout():\n\
             \x20   payments = Payments()\n\
             \x20   return payments.charge(10)\n",
        );
    repo.commit("Start the shop");
    repo.write("shop/pay.py", pay("validate(amount * 2)"));
    repo.commit("Double the charge");
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);
    session.settle();

    assert_eq!(
        connected_rows(&session),
        [
            [
                "shop/cart.py",
                "lines 4–6",
                "1 KB",
                "included",
                "calls `charge`"
            ],
            [
                "shop/check.py",
                "lines 1–2",
                "1 KB",
                "included",
                "called by `charge`"
            ],
        ]
    );
}

/// A Python module of three functions, each called from its own file, with
/// `sub` recording its result through `records.record`.
fn python_ops() -> TestRepo {
    let repo = TestRepo::new("ops");
    repo.write("ops.py", OPS)
        .write("records.py", "def record(value):\n    return [value]\n")
        .write(
            "use_add.py",
            "from ops import add\n\n\ndef twice(a):\n    return add(a, a)\n",
        )
        .write(
            "use_sub.py",
            "from ops import sub\n\n\ndef none_left(a):\n    return sub(a, a)\n",
        )
        .write(
            "use_mul.py",
            "from ops import mul\n\n\ndef square(a):\n    return mul(a, a)\n",
        );
    repo.commit("Add the operations");
    repo
}

/// `ops.py` as [`python_ops`] first commits it.
const OPS: &str = "\
from records import record


def add(a, b):
    return a + b


def sub(a, b):
    total = a - b
    total = round(total)
    record(total)
    return total


def mul(a, b):
    return a * b
";

/// The Connected code rows for a change to `sub` alone.
const SUB_CONNECTED: [[&str; 5]; 2] = [
    [
        "records.py",
        "lines 1–2",
        "1 KB",
        "included",
        "called by `sub`",
    ],
    ["use_sub.py", "lines 4–5", "1 KB", "included", "calls `sub`"],
];

/// The Connected code rows of the review of `repo`'s newest commit, once
/// found.
fn found_for_newest(repo: &TestRepo) -> Vec<Vec<String>> {
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();
    connected_rows(&session)
}

#[test]
fn a_change_inside_one_function_of_several_lists_only_that_functions_callers_and_callees() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");

    assert_eq!(found_for_newest(&repo), SUB_CONNECTED);
}

#[test]
fn a_change_that_only_removes_lines_inside_a_function_counts_as_changing_it() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("    total = round(total)\n", ""));
    repo.commit("Keep the fraction");

    assert_eq!(found_for_newest(&repo), SUB_CONNECTED);
}

#[test]
fn a_deleted_function_has_no_connected_code() {
    let repo = python_ops();
    repo.write(
        "ops.py",
        OPS.replace(
            "def sub(a, b):\n    total = a - b\n    total = round(total)\n    record(total)\n    return total\n\n\n",
            "",
        ),
    );
    repo.commit("Drop sub");
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);
    session.settle();

    assert_eq!(
        connected_code(&session),
        ["Connected code: none found for this change."]
    );
}

#[test]
fn a_caller_inside_a_touched_file_is_not_listed_since_the_file_is_sent_already() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"))
        .write(
            "use_sub.py",
            "from ops import sub\n\n\ndef none_left(a):\n    return sub(a, a) + 0\n",
        );
    repo.commit("Subtract the other way round, and say so");

    assert_eq!(found_for_newest(&repo), [SUB_CONNECTED[0]]);
}

/// `python_ops` with `callers.py`, whose `first` and `second` touch and
/// `apart` stands on its own, and a newest commit that changes `add`,
/// `sub` and `mul`.
fn with_merged_callers() -> TestRepo {
    let repo = python_ops();
    repo.write(
        "callers.py",
        "from ops import add, mul, sub\n\
         \n\
         \n\
         def first(a):\n\
         \x20   return add(a, 1)\n\
         def second(a):\n\
         \x20   return sub(mul(a, 2), 1)\n\
         \n\
         \n\
         def apart(a):\n\
         \x20   return sub(a, 2)\n",
    );
    repo.commit("Call the operations");
    repo.write(
        "ops.py",
        OPS.replace("a + b", "b + a")
            .replace("total = a - b", "total = b - a")
            .replace("a * b", "b * a"),
    );
    repo.commit("Swap the operands");
    repo
}

#[test]
fn callers_that_touch_in_one_file_are_one_range_naming_two_relations_then_how_many_more() {
    let repo = with_merged_callers();

    assert_eq!(
        found_for_newest(&repo)[..2],
        [
            [
                "callers.py",
                "lines 4–7",
                "1 KB",
                "included",
                "calls `add`, calls `mul`, +1 more"
            ],
            [
                "callers.py",
                "lines 10–11",
                "1 KB",
                "included",
                "calls `sub`"
            ],
        ]
    );
}

#[test]
fn each_touched_file_with_connected_code_says_how_many_pieces_it_has() {
    let repo = with_merged_callers();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(
        columns(&session.section("Files")),
        [["ops.py", "1 KB", "included"]]
    );

    session.settle();

    assert_eq!(connected_rows(&session).len(), 6);
    assert_eq!(
        columns(&session.section("Files")),
        [["ops.py", "1 KB", "included · 6 connected"]]
    );
}

#[test]
fn space_leaves_out_connected_code_and_brings_it_back_and_the_size_follows() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    let before = session.section("Size");
    session.settle();
    let found = session.section("Size");
    assert_ne!(found, before, "connected code counts towards the size");

    // The change, the touched file, then the first connection.
    session.press("j j <Space>");

    assert_eq!(
        connected_rows(&session)[0],
        [
            "records.py",
            "lines 1–2",
            "1 KB",
            "left out by you",
            "called by `sub`"
        ]
    );
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 1 range · 1 KB of 100 KB limit"
    );
    session.press("j <Space>");
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: none sent · 0 KB of 100 KB limit"
    );
    assert_eq!(session.section("Size"), before);

    session.press("<Space> k <Space>");
    assert_eq!(connected_rows(&session), SUB_CONNECTED);
    assert_eq!(session.section("Size"), found);
}

/// The reader's rows under its title, trimmed.
pub(crate) fn reader(session: &Session) -> Vec<String> {
    session
        .rows()
        .iter()
        .skip(3)
        .map(|row| row.trim().to_owned())
        .take_while(|row| !row.starts_with("? keys"))
        .collect()
}

#[test]
fn enter_on_connected_code_reads_exactly_the_lines_it_sends_numbered_as_in_the_file() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    let mut session = started_in(repo.root(), 120, 40);
    session.press(REVIEW_NEWEST);
    session.settle();

    session.press("j j j <Enter>");

    assert_eq!(
        session.rows()[1].trim(),
        "What is sent for use_sub.py, lines 4–5"
    );
    let rows = reader(&session);
    assert_eq!(
        rows[..6],
        [
            "Why it is sent",
            "calls `sub`",
            "",
            "Content, as it is sent",
            "4 def none_left(a):",
            "5     return sub(a, a)",
        ]
    );
    assert!(rows[6..].iter().all(String::is_empty), "{rows:#?}");

    session.press("q <Space> <Enter>");
    assert_eq!(
        reader(&session)[3..5],
        [
            "Nothing is sent",
            "use_sub.py, lines 4–5, is left out by you."
        ]
    );
}

/// A fake AWS access key ID, split so that no secret scanner finds it in
/// the source.
pub(crate) const AWS_KEY: &str = concat!("AKIA", "FAKEFAKEFAKEFAKE");

/// The Send control's row, trimmed, without the focus marker.
pub(crate) fn send(session: &Session) -> String {
    let rows = session.rows();
    let send = rows
        .iter()
        .map(|row| row.trim().trim_start_matches("> "))
        .find(|row| *row == "Send" || row.starts_with("Send ("));
    assert!(send.is_some(), "no Send:\n{}", session.text());
    send.unwrap_or_default().to_owned()
}

/// `python_ops` whose `use_sub.py` holds an AWS key in its caller of
/// `sub`, and a newest commit that changes `sub`.
fn with_a_secret_in_a_caller() -> TestRepo {
    let repo = python_ops();
    repo.write(
        "use_sub.py",
        format!(
            "from ops import sub\n\n\ndef none_left(a):\n    key = \"{AWS_KEY}\"\n    return sub(a, a)\n"
        ),
    );
    repo.commit("Sign the subtraction");
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    repo
}

#[test]
fn a_secret_in_a_caller_blocks_send_until_it_is_left_out_or_marked_as_not_a_secret() {
    let repo = with_a_secret_in_a_caller();
    let mut session = started_with_a_key_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(send(&session), "Send (finding connected code)");

    session.settle();

    assert_eq!(
        session.section("Secret check"),
        [
            "Possible secret in use_sub.py, line 5: AWS access key ID, AKIA********",
            "Leave out the connected code",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
    assert_eq!(send(&session), "Send (resolve the possible secrets)");

    // From the change round to Back, then up to the finding's first action.
    session.press("<S-Tab> k k k");
    assert_eq!(
        session.row_with("Leave out the connected code"),
        "> Leave out the connected code"
    );
    session.press("<Space>");
    assert_eq!(connected_rows(&session)[1][3], "left out by you");
    assert_eq!(send(&session), "Send");

    // Bring it back from its row, then mark the match as not a secret.
    session.press("k k k k k k <Space>");
    assert_eq!(send(&session), "Send (resolve the possible secrets)");
    session.press("k k k <S-Tab> k k <Space>");
    assert_eq!(session.section("Secret check")[2], "> (x) Not a secret");
    assert_eq!(send(&session), "Send");
}

#[test]
fn choices_on_connected_code_survive_going_back_to_the_same_commit_and_reset_for_another() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();
    session.press("j j <Space>");
    let chosen = connected_rows(&session);
    assert_eq!(chosen[0][3], "left out by you");

    // Back to the picker, and review the same commit again.
    session.press("q <Enter>");
    assert_eq!(connected_code(&session), ["Connected code: finding…"]);
    session.settle();
    assert_eq!(connected_rows(&session), chosen);

    // Choose the first commit, then the newest again, and review it.
    session.press("q k <Space> k <Space> j j <Enter>");
    assert_eq!(session.rows()[4].trim(), "Review what will be sent");
    session.settle();
    assert_eq!(connected_rows(&session), SUB_CONNECTED);
}

#[test]
fn connected_code_found_for_a_review_that_has_closed_is_never_applied_to_another() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("a + b", "b + a"));
    repo.commit("Add the other way round");
    repo.write(
        "ops.py",
        OPS.replace("a + b", "b + a")
            .replace("total = a - b", "total = b - a"),
    );
    repo.commit("Subtract the other way round");
    let mut session = started_in(repo.root(), 120, 60);
    // Review the newest commit, then go back before its connected code is
    // found and review the one before it.
    session.press(REVIEW_NEWEST);
    session.press("q k k <Space> j j <Enter>");
    assert_eq!(
        session.row_with("Add the other way round"),
        format!("> {}  Add the other way round", short_id(&repo, "HEAD~1"))
    );

    // The newest commit's connected code arrives once its review has
    // closed, and only then is the one before it found, in its turn.
    session.settle();

    assert_eq!(
        connected_rows(&session),
        [["use_add.py", "lines 4–5", "1 KB", "included", "calls `add`"]]
    );
}

/// The short ID of `repo`'s commit `revision`.
fn short_id(repo: &TestRepo, revision: &str) -> String {
    repo.git(&["log", "-1", "--format=%h", revision])
        .trim()
        .to_owned()
}

#[test]
fn when_the_analysis_fails_the_review_says_connected_code_is_not_available() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    // The engine never reads inside a `build` directory, so nothing written
    // there can be analysed.
    let temp = tempfile::TempDir::new().expect("create a temporary directory");
    let build = temp.path().join("build");
    std::fs::create_dir(&build).expect("create a build directory");
    let mut session = Session::start(
        Setup {
            variables: &[
                ("NO_COLOR", "1"),
                ("TMPDIR", build.to_str().expect("a UTF-8 path")),
            ],
            start: Some(repo.root()),
            ..Setup::default()
        },
        120,
        60,
    );
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        connected_code(&session),
        ["Connected code isn't available for this change: the analysis failed."]
    );
    assert_eq!(send(&session), "Send (needs OPENROUTER_API_KEY)");
}

#[test]
fn the_review_is_usable_while_connected_code_is_found_and_keeps_its_focus_when_it_arrives() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    // Leave out the touched file, then move on to the first area.
    session.press("j <Space> j");
    assert_eq!(session.row_with("ops.py"), "ops.py  1 KB  left out by you");
    assert_eq!(
        session.row_with("Domain"),
        "> Domain          Whetstone default"
    );
    session.settle();

    assert_eq!(
        session.row_with("Domain"),
        "> Domain          Whetstone default"
    );
    assert_eq!(connected_rows(&session), SUB_CONNECTED);
}

#[test]
fn callers_in_secret_paths_or_in_files_the_repository_excludes_are_never_connected_code() {
    let repo = python_ops();
    repo.write(
        "credentials.py",
        "from ops import sub\n\n\ndef token(a):\n    return sub(a, 1)\n",
    )
    .write(
        "fixture.py",
        "from ops import sub\n\n\ndef sample(a):\n    return sub(a, 2)\n",
    );
    repo.commit("Subtract in more places");
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    repo.commit("Subtract the other way round");
    repo.write(".whetstone/config.toml", "exclude = [\"fixture.py\"]\n");

    assert_eq!(found_for_newest(&repo), SUB_CONNECTED);
}

/// Everything recorded in the log while `run` runs, at every level.
pub(crate) fn logged(run: impl FnOnce()) -> String {
    use std::io;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("log lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = captured.0.lock().expect("log lock").clone();
    String::from_utf8(bytes).expect("the log is UTF-8")
}

#[test]
fn a_session_finding_connected_code_writes_nothing_leaves_no_analysis_directory_and_logs_no_code() {
    let repo = with_a_secret_in_a_caller();
    let before = repo.on_disk();
    let temp = tempfile::TempDir::new().expect("create a system temporary directory");

    let log = logged(|| {
        let mut session = Session::start(
            Setup {
                variables: &[
                    ("NO_COLOR", "1"),
                    ("TMPDIR", temp.path().to_str().expect("a UTF-8 path")),
                ],
                start: Some(repo.root()),
                ..Setup::default()
            },
            80,
            24,
        );
        session.press(REVIEW_NEWEST);
        session.settle();
        // Read both pieces of connected code, leave one out, then review the
        // commit again and quit while its connected code is being found.
        session.press("j j <Enter> q j <Enter> q <Space> q <Enter>");
        assert_eq!(session.press("<C-c>"), whetstone::Flow::Quit);
    });

    repo.assert_untouched(&before);
    let left: Vec<_> = std::fs::read_dir(temp.path())
        .expect("read the temporary directory")
        .collect();
    assert!(left.is_empty(), "left in the temporary directory: {left:?}");
    assert_eq!(
        log.matches("found a commit's connected code").count(),
        2,
        "one analysis each time the review opens: {log}"
    );
    assert!(log.contains("applied connected code"), "{log}");
    for unlogged in [
        AWS_KEY,
        "none_left",
        "`sub`",
        "record(total)",
        "use_sub.py",
        "records.py",
    ] {
        assert!(!log.contains(unlogged), "{unlogged:?} in the log:\n{log}");
    }
}

#[test]
fn connected_code_in_the_review_at_120x32() {
    let repo = with_merged_callers();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.settle();
    // Leave out the first piece, whose relations then go under its row in
    // a narrow window.
    session.press("j j <Space>");

    insta::assert_debug_snapshot!("connected_code_120x32", session.frame());
}

#[test]
fn connected_code_in_the_review_at_80x24() {
    let repo = with_merged_callers();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);
    session.settle();
    // Leave out the first piece, whose relations then go under its row in
    // a narrow window.
    session.press("j j <Space>");

    insta::assert_debug_snapshot!("connected_code_80x24", session.frame());
}

#[test]
fn reading_connected_code_at_120x32() {
    let repo = with_merged_callers();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.settle();
    session.press("j j <Enter>");

    insta::assert_debug_snapshot!("reading_connected_code_120x32", session.frame());
}

#[test]
fn reading_connected_code_at_80x24() {
    let repo = with_merged_callers();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);
    session.settle();
    session.press("j j <Enter>");

    insta::assert_debug_snapshot!("reading_connected_code_80x24", session.frame());
}
