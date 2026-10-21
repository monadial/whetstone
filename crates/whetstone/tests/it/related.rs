//! Related tests and related configuration in the outbound review: the
//! tests that conventionally belong to each touched file and its nearest
//! build manifest, found with the connected code once the review opens,
//! and listed, read and left out like it.

use crate::connected_code::{AWS_KEY, REVIEW_NEWEST, connected_code, connected_rows, reader, send};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, Setup, columns};

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
                "web/src/__tests__/cart.test.ts",
                "web/src/__tests__/cart.spec.js",
                "web/src/__tests__/cart.json",
                "web/src/__tests__/cart.fixture.ts",
                "web/src/cart.tests.ts",
            ],
        ),
        [
            ["web/package.json", "manifest for `web/src/cart.ts`"],
            [
                "web/src/__tests__/cart.spec.js",
                "test for `web/src/cart.ts`"
            ],
            [
                "web/src/__tests__/cart.test.ts",
                "test for `web/src/cart.ts`"
            ],
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
fn of_two_manifests_in_one_directory_a_file_gets_its_own_ecosystems_else_the_first_listed() {
    assert_eq!(
        related_to_changing(
            &["web/index.ts", "web/README.md"],
            &["web/Cargo.toml", "web/package.json"],
        ),
        [
            ["web/Cargo.toml", "manifest for `web/README.md`"],
            ["web/package.json", "manifest for `web/index.ts`"],
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

/// A Python shop whose `cart.py`, `credentials.py`, `pay.py` and
/// `store.py` each have a test: `credentials_test.py` at a possible secret
/// path, `test_pay.py` a symbolic link to `test_cart.py`, and
/// `test_store.py` excluded by the repository's config. Its newest commit
/// changes all four.
fn with_a_test_of_each_status() -> TestRepo {
    let repo = TestRepo::new("shop");
    for name in ["cart", "credentials", "pay", "store"] {
        let path = format!("shop/{name}.py");
        repo.write(&path, placeholder(&path, 1));
    }
    repo.write(
        "shop/credentials_test.py",
        "def test_credentials():\n    pass\n",
    )
    .write("shop/test_cart.py", "def test_cart():\n    pass\n")
    .write("shop/test_store.py", "def test_store():\n    pass\n")
    .symlink("shop/test_pay.py", "test_cart.py");
    repo.commit("Start the shop");
    for name in ["cart", "credentials", "pay", "store"] {
        let path = format!("shop/{name}.py");
        repo.write(&path, placeholder(&path, 2));
    }
    repo.commit("Change the shop");
    repo.write(
        ".whetstone/config.toml",
        "exclude = [\"shop/test_store.py\"]\n",
    );
    repo
}

/// The path and status of each Related tests and configuration row.
fn statuses(session: &Session) -> Vec<[String; 2]> {
    related_rows(session)
        .into_iter()
        .map(|row| [row[0].clone(), row[3].clone()])
        .collect()
}

#[test]
fn related_tests_are_left_out_and_brought_back_by_the_same_rules_and_choices_as_connected_code() {
    let repo = with_a_test_of_each_status();
    let mut session = reviewed(&repo);
    assert_eq!(
        statuses(&session),
        [
            ["shop/credentials_test.py", "left out: possible secret path"],
            ["shop/test_cart.py", "included"],
            ["shop/test_pay.py", "left out: symlink"],
            [
                "shop/test_store.py",
                "left out by repository config: shop/test_store.py"
            ],
        ]
    );
    assert_eq!(
        related(&session)[0],
        "Related tests and configuration: 1 test · 1 KB"
    );

    // Past the change and the four touched files, bring back the test at a
    // secret path, then leave out the one included.
    session.press("j j j j j <Space> j <Space>");

    assert_eq!(
        statuses(&session)[..2],
        [
            ["shop/credentials_test.py", "included by you"],
            ["shop/test_cart.py", "left out by you"],
        ]
    );
    assert_eq!(
        related(&session)[0],
        "Related tests and configuration: 1 test · 1 KB"
    );
    // A symbolic link stays left out; a file the repository excludes comes
    // back.
    session.press("j <Space> j <Space>");
    assert_eq!(
        statuses(&session)[2..],
        [
            ["shop/test_pay.py", "left out: symlink"],
            ["shop/test_store.py", "included by you"],
        ]
    );
}

#[test]
fn a_secret_in_a_related_test_blocks_send_until_the_test_is_left_out() {
    let repo = TestRepo::new("shop");
    repo.write("store.go", placeholder("store.go", 1)).write(
        "store_test.go",
        format!("package store\n\nconst key = \"{AWS_KEY}\"\n"),
    );
    repo.commit("Start the shop");
    repo.write("store.go", placeholder("store.go", 2));
    repo.commit("Change the store");
    let mut session = reviewed(&repo);

    assert_eq!(
        session.section("Secret check"),
        [
            "Possible secret in store_test.go, line 3: AWS access key ID, AKIA********",
            "Leave out the related test",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
    assert_eq!(send(&session), "Send (resolve the possible secrets)");

    // From the change round to Back, then up to the finding's first action.
    session.press("<S-Tab> k k k <Space>");

    assert_eq!(statuses(&session), [["store_test.go", "left out by you"]]);
    assert_eq!(send(&session), "Send (needs OPENROUTER_API_KEY)");
}

/// A Python shop whose `store.save` is called by `cart.checkout` and by
/// its own test, with a newest commit that changes `save`.
fn with_a_test_that_calls_the_change() -> TestRepo {
    let repo = TestRepo::new("shop");
    let store = |body: &str| format!("def save(order):\n    return {body}\n");
    repo.write("shop/__init__.py", "")
        .write("shop/store.py", store("order"))
        .write(
            "shop/cart.py",
            "from shop.store import save\n\n\ndef checkout(order):\n    return save(order)\n",
        )
        .write(
            "shop/test_store.py",
            "from shop.store import save\n\n\ndef test_save():\n    assert save(1) == 1\n",
        );
    repo.commit("Start the shop");
    repo.write("shop/store.py", store("[order]"));
    repo.commit("Save a list");
    repo
}

#[test]
fn a_related_test_that_calls_the_change_is_sent_whole_once_as_a_test_that_calls_it() {
    let repo = with_a_test_that_calls_the_change();

    let session = reviewed(&repo);

    assert_eq!(
        related(&session),
        [
            "Related tests and configuration: 1 test · 1 KB",
            "shop/test_store.py  whole file  1 KB  included  \
             test for `shop/store.py`, calls `save`",
        ]
    );
    assert_eq!(
        connected_rows(&session),
        [[
            "shop/cart.py",
            "lines 4–5",
            "1 KB",
            "included",
            "calls `save`"
        ]]
    );
    // The test is related, not connected code.
    assert_eq!(
        session.row_with("shop/store.py  "),
        "shop/store.py  1 KB  included · 1 connected"
    );
}

#[test]
fn a_related_test_the_change_also_names_is_sent_whole_once_as_a_test_with_every_relation() {
    let repo = with_a_test_that_calls_the_change();
    repo.write(
        "shop/store.py",
        "def save(order):\n    # Tested in shop/test_store.py.\n    return [order]\n",
    );
    repo.commit("Say where saving is tested");
    let mut session = reviewed(&repo);

    assert_eq!(
        related(&session),
        [
            "Related tests and configuration: 1 test · 1 KB",
            "shop/test_store.py  whole file  1 KB  included  \
             test for `shop/store.py`, calls `save`, +1 more",
        ]
    );
    assert_eq!(
        connected_rows(&session),
        [[
            "shop/cart.py",
            "lines 4–5",
            "1 KB",
            "included",
            "calls `save`"
        ]]
    );
    // The change, the touched file, the caller, then the test.
    session.press("j j j <Enter>");
    assert_eq!(
        reader(&session)[..4],
        [
            "Why it is sent",
            "test for `shop/store.py`",
            "calls `save`",
            "named in `shop/store.py`",
        ]
    );
}

#[test]
fn choices_on_related_tests_survive_going_back_to_the_same_commit_and_reset_for_another() {
    let repo = with_a_test_that_calls_the_change();
    let mut session = reviewed(&repo);
    // The change, the touched file, the caller, then the test.
    session.press("j j j <Space>");
    let chosen = [["shop/test_store.py", "left out by you"]];
    assert_eq!(statuses(&session), chosen);

    // Back to the picker, and review the same commit again.
    session.press("q <Enter>");
    assert_eq!(
        related(&session),
        ["Related tests and configuration: finding…"]
    );
    session.settle();
    assert_eq!(statuses(&session), chosen);

    // Choose the first commit, then the newest again, and review it.
    session.press("q k <Space> k <Space> j j <Enter>");
    session.settle();
    assert_eq!(statuses(&session), [["shop/test_store.py", "included"]]);
}

#[test]
fn enter_on_a_related_test_reads_the_whole_file_it_sends_and_why() {
    let repo = with_a_test_that_calls_the_change();
    let mut session = started_in(repo.root(), 120, 40);
    session.press(REVIEW_NEWEST);
    session.settle();

    // The change, the touched file, the caller, then the test.
    session.press("j j j <Enter>");

    assert_eq!(
        session.rows()[1].trim(),
        "What is sent for shop/test_store.py, whole file"
    );
    let rows = reader(&session);
    assert_eq!(
        rows[..10],
        [
            "Why it is sent",
            "test for `shop/store.py`",
            "calls `save`",
            "",
            "Content, as it is sent",
            "1 from shop.store import save",
            "2",
            "3",
            "4 def test_save():",
            "5     assert save(1) == 1",
        ]
    );
    assert!(rows[10..].iter().all(String::is_empty), "{rows:#?}");
}

#[test]
fn a_related_test_with_no_room_under_the_size_limit_is_counted_in_its_own_section() {
    let repo = TestRepo::new("shop");
    let notes = |first: &str| {
        let line = format!("// {}\n", "x".repeat(96));
        format!("// {first}\n{}", line.repeat(3_700))
    };
    repo.write("go.mod", placeholder("go.mod", 1))
        .write("store/notes.go", notes("one"))
        .write(
            "store/notes_test.go",
            format!("// {}\n", "x".repeat(40_000)),
        );
    repo.commit("Start the shop");
    repo.write("store/notes.go", notes("two"));
    repo.commit("Change the notes");

    let session = reviewed(&repo);

    assert_eq!(
        related(&session),
        [
            "Related tests and configuration: 1 manifest · 1 KB",
            "go.mod  whole file  1 KB  included  manifest for `store/notes.go`",
            "1 more not sent: no room under the size limit",
        ]
    );
    assert_eq!(
        connected_code(&session),
        ["Connected code: none found for this change."]
    );
}

#[test]
fn when_the_analysis_fails_the_review_says_related_tests_and_configuration_are_not_available() {
    let repo = TestRepo::new("shop");
    repo.write("go.mod", placeholder("go.mod", 1))
        .write("store.go", placeholder("store.go", 1));
    repo.commit("Start the shop");
    repo.write("store.go", placeholder("store.go", 2));
    repo.commit("Change the store");
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
        related(&session),
        ["Related tests and configuration aren't available for this change: the analysis failed."]
    );
}

/// The review of [`with_a_test_that_calls_the_change`] with a
/// `pyproject.toml`, in a `width` by `height` terminal, once found, with
/// focus on the related test.
fn related_in_view(width: u16, height: u16) -> Session {
    let repo = with_a_test_that_calls_the_change();
    repo.write("pyproject.toml", "[project]\nname = \"shop\"\n");
    repo.commit("Name the shop");
    repo.write(
        "shop/store.py",
        "def save(order):\n    return [order, order]\n",
    );
    repo.commit("Save it twice");
    let mut session = started_in(repo.root(), width, height);
    session.press(REVIEW_NEWEST);
    session.settle();
    // The change, the touched file, the caller, the manifest, then the
    // test, whose relations go under its row in a narrow window.
    session.press("j j j j");
    session
}

#[test]
fn related_tests_and_configuration_in_the_review_at_120x32() {
    let session = related_in_view(120, 32);

    insta::assert_debug_snapshot!("related_120x32", session.frame());
}

#[test]
fn related_tests_and_configuration_in_the_review_at_80x24() {
    let session = related_in_view(80, 24);

    insta::assert_debug_snapshot!("related_80x24", session.frame());
}
