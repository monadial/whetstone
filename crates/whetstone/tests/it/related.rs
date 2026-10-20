//! Related tests and related configuration in the outbound review: the
//! tests that conventionally belong to each touched file and its nearest
//! build manifest, found with the connected code once the review opens,
//! and listed, read and left out like it.

use crate::connected_code::REVIEW_NEWEST;
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, columns};

/// The Related tests and configuration section: its summary, then its
/// rows, trimmed, up to the next blank row.
fn related(session: &Session) -> Vec<String> {
    let rows = session.rows();
    let start = rows
        .iter()
        .position(|row| row.trim().starts_with("Related tests and configuration"));
    assert!(
        start.is_some(),
        "no Related tests and configuration:\n{}",
        session.text()
    );
    rows.iter()
        .skip(start.unwrap_or_default())
        .map(|row| row.trim().to_owned())
        .take_while(|row| !row.is_empty())
        .collect()
}

/// The Related tests and configuration section's rows after its summary,
/// split into their columns, without the focus marker.
fn related_rows(session: &Session) -> Vec<Vec<String>> {
    columns(&related(session)[1..])
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|column| column.trim_start_matches("> ").to_owned())
                .collect()
        })
        .collect()
}

/// The review of `repo`'s newest commit, once its connected code is found.
fn reviewed(repo: &TestRepo) -> Session {
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();
    session
}

#[test]
fn a_go_change_lists_its_test_in_the_same_directory_and_the_nearest_manifest_once_found() {
    let repo = TestRepo::new("shop");
    repo.write("go.mod", "module shop\n\ngo 1.22\n")
        .write(
            "store/store.go",
            "package store\n\nfunc Open() int {\n\treturn 1\n}\n",
        )
        .write(
            "store/store_test.go",
            "package store\n\nimport \"testing\"\n\nfunc TestNothing(t *testing.T) {}\n",
        )
        .write("store/other_test.go", "package store\n");
    repo.commit("Open the store");
    repo.write(
        "store/store.go",
        "package store\n\nfunc Open() int {\n\treturn 2\n}\n",
    );
    repo.commit("Open the store twice");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(
        related(&session),
        ["Related tests and configuration: finding…"]
    );

    session.settle();

    assert_eq!(
        related(&session),
        [
            "Related tests and configuration: 1 test and 1 manifest · 1 KB",
            "go.mod               whole file  1 KB  included  manifest for `store/store.go`",
            "store/store_test.go  whole file  1 KB  included  test for `store/store.go`",
        ]
    );
}
