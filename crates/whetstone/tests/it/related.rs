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

/// The review of `repo`'s newest commit, once its connected code is
/// found, wide enough that no row's relations go under it.
fn reviewed(repo: &TestRepo) -> Session {
    let mut session = started_in(repo.root(), 200, 60);
    session.press(REVIEW_NEWEST);
    session.settle();
    session
}

/// A placeholder for the file at `path` in its `version`: a comment, or
/// an object for JSON, so that every file parses.
fn placeholder(path: &str, version: u32) -> String {
    let extension = path.rsplit('.').next().unwrap_or_default();
    match extension {
        "json" => format!("{{\"version\": {version}}}\n"),
        "xml" => format!("<!-- {version} -->\n"),
        "rs" | "go" | "js" | "jsx" | "ts" | "tsx" | "java" | "kt" | "kts" | "gradle" | "mod" => {
            format!("// {version}\n")
        }
        _ => format!("# {version}\n"),
    }
}

/// The Related tests and configuration rows, each as its path and
/// relations, of a commit that changes each of `touched`, in a repository
/// that also holds `files`, every one a placeholder.
fn related_to_changing(touched: &[&str], files: &[&str]) -> Vec<[String; 2]> {
    let repo = TestRepo::new("shop");
    for path in touched.iter().chain(files) {
        repo.write(path, placeholder(path, 1));
    }
    repo.commit("Start the shop");
    for path in touched {
        repo.write(path, placeholder(path, 2));
    }
    repo.commit("Change the shop");
    let session = reviewed(&repo);
    related_rows(&session)
        .into_iter()
        .map(|row| [row[0].clone(), row[4].clone()])
        .collect()
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

#[test]
fn a_rust_change_lists_the_test_beside_its_packages_manifest_which_is_the_nearest() {
    assert_eq!(
        related_to_changing(
            &["crates/shop/src/store.rs", "README.md"],
            &[
                "Cargo.toml",
                "crates/shop/Cargo.toml",
                "crates/shop/tests/store.rs",
                "crates/shop/src/tests/store.rs",
                "tests/store.rs",
            ],
        ),
        [
            ["Cargo.toml", "manifest for `README.md`"],
            [
                "crates/shop/Cargo.toml",
                "manifest for `crates/shop/src/store.rs`"
            ],
            [
                "crates/shop/tests/store.rs",
                "test for `crates/shop/src/store.rs`"
            ],
        ]
    );
}

#[test]
fn a_python_change_lists_its_tests_in_its_directory_and_under_an_ancestors_tests_directory() {
    assert_eq!(
        related_to_changing(
            &["src/shop/store.py"],
            &[
                "pyproject.toml",
                "src/shop/test_store.py",
                "tests/unit/store_test.py",
                "src/test/test_store.py",
                "other/tests/test_store.py",
                "src/shop/test_cart.py",
            ],
        ),
        [
            ["pyproject.toml", "manifest for `src/shop/store.py`"],
            ["src/shop/test_store.py", "test for `src/shop/store.py`"],
            ["src/test/test_store.py", "test for `src/shop/store.py`"],
            ["tests/unit/store_test.py", "test for `src/shop/store.py`"],
        ]
    );
}

#[test]
fn a_typescript_change_lists_its_test_and_spec_files_and_those_under_tests() {
    assert_eq!(
        related_to_changing(
            &["web/src/cart.ts"],
            &[
                "package.json",
                "web/package.json",
                "web/src/cart.test.ts",
                "web/src/cart.spec.js",
                "web/src/__tests__/cart.tsx",
                "web/src/__tests__/cart.json",
                "web/src/cart.tests.ts",
            ],
        ),
        [
            ["web/package.json", "manifest for `web/src/cart.ts`"],
            ["web/src/__tests__/cart.tsx", "test for `web/src/cart.ts`"],
            ["web/src/cart.spec.js", "test for `web/src/cart.ts`"],
            ["web/src/cart.test.ts", "test for `web/src/cart.ts`"],
        ]
    );
}

#[test]
fn java_and_kotlin_changes_list_their_tests_under_the_matching_test_path() {
    assert_eq!(
        related_to_changing(
            &[
                "src/main/java/shop/Store.java",
                "src/main/kotlin/shop/Cart.kt"
            ],
            &[
                "pom.xml",
                "src/test/java/shop/StoreTest.java",
                "src/test/java/shop/StoreTests.java",
                "src/test/java/shop/TestStore.java",
                "src/test/java/shop/StoreIT.kt",
                "src/test/java/StoreTest.java",
                "src/test/kotlin/shop/CartTest.kt",
            ],
        ),
        [
            [
                "pom.xml",
                "manifest for `src/main/java/shop/Store.java`, \
                 manifest for `src/main/kotlin/shop/Cart.kt`"
            ],
            [
                "src/test/java/shop/StoreIT.kt",
                "test for `src/main/java/shop/Store.java`"
            ],
            [
                "src/test/java/shop/StoreTest.java",
                "test for `src/main/java/shop/Store.java`"
            ],
            [
                "src/test/java/shop/StoreTests.java",
                "test for `src/main/java/shop/Store.java`"
            ],
            [
                "src/test/java/shop/TestStore.java",
                "test for `src/main/java/shop/Store.java`"
            ],
            [
                "src/test/kotlin/shop/CartTest.kt",
                "test for `src/main/kotlin/shop/Cart.kt`"
            ],
        ]
    );
}

#[test]
fn a_ruby_change_lists_its_spec_and_its_test() {
    assert_eq!(
        related_to_changing(
            &["lib/shop/store.rb"],
            &[
                "Gemfile",
                "spec/shop/store_spec.rb",
                "test/store_test.rb",
                "lib/shop/store_spec.rb",
            ],
        ),
        [
            ["Gemfile", "manifest for `lib/shop/store.rb`"],
            ["spec/shop/store_spec.rb", "test for `lib/shop/store.rb`"],
            ["test/store_test.rb", "test for `lib/shop/store.rb`"],
        ]
    );
}

#[test]
fn an_elixir_change_lists_its_test_under_test() {
    assert_eq!(
        related_to_changing(
            &["lib/shop/store.ex"],
            &[
                "mix.exs",
                "test/shop/store_test.exs",
                "test/shop/cart_test.exs"
            ],
        ),
        [
            ["mix.exs", "manifest for `lib/shop/store.ex`"],
            ["test/shop/store_test.exs", "test for `lib/shop/store.ex`"],
        ]
    );
}

#[test]
fn a_touched_test_has_no_related_tests_of_its_own_but_still_its_manifest() {
    assert_eq!(
        related_to_changing(&["store/store_test.go"], &["go.mod", "store/store.go"],),
        [["go.mod", "manifest for `store/store_test.go`"]]
    );
}

#[test]
fn a_touched_manifest_is_never_related_and_stays_the_nearest_for_its_package() {
    assert_eq!(
        related_to_changing(
            &["crates/shop/Cargo.toml", "crates/shop/src/store.rs"],
            &["Cargo.toml", "crates/shop/tests/store.rs"],
        ),
        [[
            "crates/shop/tests/store.rs",
            "test for `crates/shop/src/store.rs`"
        ]]
    );
}

#[test]
fn a_deleted_file_has_no_related_tests_or_configuration() {
    let repo = TestRepo::new("shop");
    repo.write("go.mod", placeholder("go.mod", 1))
        .write("store/store.go", placeholder("store/store.go", 1))
        .write("store/store_test.go", placeholder("store/store_test.go", 1));
    repo.commit("Start the shop");
    std::fs::remove_file(repo.path("store/store.go")).expect("delete the store");
    repo.commit("Drop the store");

    assert_eq!(
        related(&reviewed(&repo)),
        ["Related tests and configuration: none found for this change."]
    );
}
