//! Connected code from what a change links to and names: the repository
//! files that touched Markdown links to on changed lines, and the
//! repository paths that changed lines name, each sent whole.

use crate::connected_code::{REVIEW_NEWEST, connected_code, connected_rows, logged};
use crate::repo::TestRepo;
use crate::repository::started_in;

/// A course of documents, with no code: a README, two lessons and an
/// exercise.
fn course() -> TestRepo {
    let repo = TestRepo::new("course");
    repo.write("README.md", "# Course\n")
        .write("lessons/intro.md", "# Intro\n\nWelcome.\n")
        .write("lessons/loops.md", "# Loops\n\nCount to ten.\n")
        .write("exercises/loops.txt", "Print 1 to 10.\n");
    repo.commit("Start the course");
    repo
}

#[test]
fn a_repository_of_documents_gets_connected_code_from_links_and_mentions_alone() {
    let repo = course();
    repo.write(
        "README.md",
        "# Course\n\nStart with [the intro](lessons/intro.md).\n\nThen try exercises/loops.txt.\n",
    );
    repo.commit("Point to the first lesson and exercise");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 files · 1 KB"
    );
    assert_eq!(
        connected_rows(&session),
        [
            [
                "exercises/loops.txt",
                "whole file",
                "1 KB",
                "included",
                "named in `README.md`"
            ],
            [
                "lessons/intro.md",
                "whole file",
                "1 KB",
                "included",
                "linked from `README.md`"
            ],
        ]
    );
}

#[test]
fn links_resolve_against_the_linking_file_or_the_root_and_only_files_in_the_commit_count() {
    let repo = course();
    repo.write(
        "lessons/loops.md",
        "# Loops\n\
         \n\
         Count to ten.\n\
         \n\
         - [Back to the intro](intro.md#welcome)\n\
         - [The exercise](../exercises/loops.txt?plain=1)\n\
         - [The course](/README.md)\n\
         - [More on loops](https://example.com/loops.md)\n\
         - [The solution](solutions/loops.md)\n\
         - [Every exercise](../exercises/)\n\
         - [Every lesson](/lessons)\n",
    );
    repo.commit("Link the loops lesson");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        connected_rows(&session),
        [
            [
                "README.md",
                "whole file",
                "1 KB",
                "included",
                "linked from `lessons/loops.md`"
            ],
            [
                "exercises/loops.txt",
                "whole file",
                "1 KB",
                "included",
                "linked from `lessons/loops.md`"
            ],
            [
                "lessons/intro.md",
                "whole file",
                "1 KB",
                "included",
                "linked from `lessons/loops.md`"
            ],
        ]
    );
}

#[test]
fn a_path_a_changed_line_names_is_connected_code_but_a_bare_file_name_or_a_longer_path_is_not() {
    let repo = course();
    repo.write("scripts/check.sh", "#!/bin/sh\necho ok\n")
        .write("notes.txt", "Start with lessons/intro.md.\n");
    repo.commit("Add a check and notes");
    repo.write(
        "notes.txt",
        "Start with lessons/intro.md.\n\
         Run ./scripts/check.sh before you hand in.\n\
         The exercise is in loops.txt.\n\
         Old copies live in archive/exercises/loops.txt.\n",
    );
    repo.commit("Say how to check");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        connected_rows(&session),
        [[
            "scripts/check.sh",
            "whole file",
            "1 KB",
            "included",
            "named in `notes.txt`"
        ]]
    );
}

/// A Python shop with documentation, whose newest commit changes
/// `Payments.charge`, names its documentation and its caller's file in it,
/// and links to the documentation from the README.
fn documented_shop() -> TestRepo {
    let repo = TestRepo::new("shop");
    let pay = |body: &str| {
        format!(
            "from shop.check import validate\n\
             \n\
             \n\
             class Payments:\n\
             \x20   def charge(self, amount):\n\
             {body}"
        )
    };
    repo.write("README.md", "# Shop\n")
        .write("docs/payments.md", "# Payments\n\nCharges are doubled.\n")
        .write("shop/__init__.py", "")
        .write(
            "shop/check.py",
            "def validate(amount):\n    return amount > 0\n",
        )
        .write("shop/pay.py", pay("        return validate(amount)\n"))
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
    repo.write(
        "shop/pay.py",
        pay(
            "        # See docs/payments.md, and shop/cart.py for the checkout.\n\
             \x20       return validate(amount * 2)\n",
        ),
    )
    .write(
        "README.md",
        "# Shop\n\nHow [payments](docs/payments.md) work.\n",
    );
    repo.commit("Double the charge");
    repo
}

#[test]
fn a_file_found_more_than_once_is_sent_whole_once_with_every_relation() {
    let repo = documented_shop();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 1 range and 2 files · 1 KB"
    );
    assert_eq!(
        connected_rows(&session),
        [
            [
                "docs/payments.md",
                "whole file",
                "1 KB",
                "included",
                "linked from `README.md`, named in `shop/pay.py`"
            ],
            [
                "shop/cart.py",
                "whole file",
                "1 KB",
                "included",
                "calls `charge`, named in `shop/pay.py`"
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

#[test]
fn connected_code_from_links_and_mentions_in_the_review_at_120x32() {
    let repo = documented_shop();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.settle();

    insta::assert_debug_snapshot!("links_and_mentions_120x32", session.frame());
}

#[test]
fn connected_code_from_links_and_mentions_in_the_review_at_80x24() {
    let repo = documented_shop();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);
    session.settle();
    // Down to the first piece of connected code, past the two touched files.
    session.press("j j j");

    insta::assert_debug_snapshot!("links_and_mentions_80x24", session.frame());
}

#[test]
fn a_linked_symlink_or_a_named_binary_file_is_listed_but_always_left_out() {
    let repo = course();
    repo.symlink("lessons/latest.md", "loops.md")
        .write("images/loop.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    repo.commit("Add the latest lesson and a picture");
    repo.write(
        "README.md",
        "# Course\n\nRead [the latest lesson](lessons/latest.md), drawn in images/loop.png.\n",
    );
    repo.commit("Point to the latest lesson");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        connected_rows(&session),
        [
            [
                "images/loop.png",
                "whole file",
                "1 KB",
                "left out: binary file",
                "named in `README.md`"
            ],
            [
                "lessons/latest.md",
                "whole file",
                "–",
                "left out: symlink",
                "linked from `README.md`"
            ],
        ]
    );
}

#[test]
fn finding_what_a_change_links_to_and_names_logs_counts_but_never_the_files() {
    let repo = course();
    repo.write(
        "README.md",
        "# Course\n\nStart with [the intro](lessons/intro.md).\n\nThen try exercises/loops.txt.\n",
    );
    repo.commit("Point to the first lesson and exercise");

    let log = logged(|| {
        let mut session = started_in(repo.root(), 80, 24);
        session.press(REVIEW_NEWEST);
        session.settle();
    });

    assert!(
        log.contains("found the files a change links to and names linked=1 named=2"),
        "{log}"
    );
    for unlogged in ["lessons/intro.md", "exercises/loops.txt", "Welcome"] {
        assert!(!log.contains(unlogged), "{unlogged:?} in the log:\n{log}");
    }
}
