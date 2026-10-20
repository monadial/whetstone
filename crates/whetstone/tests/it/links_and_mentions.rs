//! Connected code from what a change links to and names: the repository
//! files that touched Markdown links to on changed lines, and the
//! repository paths that changed lines name, each sent whole.

use crate::connected_code::{REVIEW_NEWEST, connected_code, connected_rows};
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
