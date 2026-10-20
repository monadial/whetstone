//! Tests through the module's interface: [`find`] on small repositories,
//! the analysis directory it writes, and the startup sweep.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tempfile::TempDir;
use whetstone_core::{Change, CommitId, ExcludePatterns};

use super::*;
use crate::repository::test_repo::{TestRepo, files_under};

/// The connected code `find` finds for the commit `id` of `repo`, with an
/// empty cache, each connection described as its path, lines, tier and
/// relations, after checking that it left nothing in the temporary
/// directory.
fn connected(repo: &TestRepo, id: CommitId) -> Vec<String> {
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");

    let found = find_in(repo, id, temp.path(), &cache_in(cache.path()));

    assert_eq!(
        files_under(temp.path()),
        Vec::<String>::new(),
        "left behind"
    );
    described(&found)
}

/// What `find` finds for the commit `id` of `repo`, writing any files it
/// analyses in `temp_dir` and caching its graph in `cache`.
fn find_in(repo: &TestRepo, id: CommitId, temp_dir: &Path, cache: &GraphCache) -> Found {
    let (_, found) = found_in(repo, id, temp_dir, cache);
    found
}

/// The graph cache in the cache folder `folder`, as the app opens it.
fn cache_in(folder: &Path) -> GraphCache {
    GraphCache::open(folder, GraphCache::CAP)
}

/// The connections of `found`, each described as [`describe`] does.
fn described(found: &Found) -> Vec<String> {
    found.connections().iter().map(describe).collect()
}

/// The change of the commit `id` of `repo`, and what `find` finds for it,
/// analysed in `temp` and cached in `cache`.
fn found_in(repo: &TestRepo, id: CommitId, temp: &Path, cache: &GraphCache) -> (Change, Found) {
    let repository = repo.open();
    let change = repository.load(id).expect("load the change");
    let found = repository
        .handle()
        .with_tree(id, |tree| {
            find(
                tree,
                &change,
                &ExcludePatterns::default(),
                &AnalysisDirs::new(temp.to_path_buf()),
                cache,
            )
        })
        .expect("read the commit's tree")
        .expect("find connected code");
    (change, found)
}

/// A connection as `path start-end tier: relation in touched file, …`.
fn describe(connection: &Connection) -> String {
    let tier = match connection.tier() {
        Tier::High => "high",
        Tier::Medium => "medium",
    };
    let relations = connection
        .relations()
        .iter()
        .map(|relation| match relation {
            Relation::Calls {
                callable,
                touched_file,
            } => format!("calls {callable} in {touched_file}"),
            Relation::CalledBy {
                callable,
                touched_file,
            } => format!("called by {callable} in {touched_file}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} {}-{} {tier}: {relations}",
        connection.path(),
        connection.lines().start(),
        connection.lines().end(),
    )
}

/// A snapshot held in memory, as the showcase project can make one.
#[derive(Debug, Default)]
struct Memory(BTreeMap<String, (FileKind, Vec<u8>)>);

impl Memory {
    fn with(mut self, path: &str, kind: FileKind, contents: impl Into<Vec<u8>>) -> Self {
        self.0.insert(path.to_owned(), (kind, contents.into()));
        self
    }
}

impl Snapshot for Memory {
    type Error = Infallible;

    fn id(&self) -> SnapshotId {
        SnapshotId::Showcase(0)
    }

    fn files(&self) -> Result<Vec<SnapshotFile>, Infallible> {
        Ok(self
            .0
            .iter()
            .map(|(path, (kind, _))| SnapshotFile::new(path.clone(), *kind))
            .collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, Infallible> {
        Ok(self.0.get(path).and_then(|(kind, contents)| {
            matches!(kind, FileKind::Blob | FileKind::Executable).then(|| contents.clone())
        }))
    }
}

/// A Rust crate in which `Payments::charge` validates with
/// `check::validate` and is called by `shop::checkout`.
fn rust_payments() -> TestRepo {
    let repo = TestRepo::new();
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
fn rust_pay(body: &str) -> String {
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
fn a_rust_change_connects_to_the_callers_and_callees_of_the_method_it_modifies() {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    let id = repo.commit("Double the charge");

    assert_eq!(
        connected(&repo, id),
        [
            "src/check.rs 1-3 high: called by charge in src/pay.rs",
            "src/shop.rs 3-5 high: calls charge in src/pay.rs",
        ]
    );
}

/// [`rust_payments`] with a commit that changes `Payments::charge`, whose
/// connected code is [`CHARGE_CONNECTED`].
fn double_the_charge() -> (TestRepo, CommitId) {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    let id = repo.commit("Double the charge");
    (repo, id)
}

/// What finding connected code gives for [`double_the_charge`].
const CHARGE_CONNECTED: [&str; 2] = [
    "src/check.rs 1-3 high: called by charge in src/pay.rs",
    "src/shop.rs 3-5 high: calls charge in src/pay.rs",
];

#[test]
fn finding_a_commits_connected_code_again_takes_its_graph_from_the_cache_without_analysing() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");
    let analysed = find_in(&repo, id, temp.path(), &cache_in(cache.path()));
    // Where files would be written for analysis. It does not exist, so
    // writing any there would fail.
    let nowhere = temp.path().join("nowhere");

    let cached = find_in(&repo, id, &nowhere, &cache_in(cache.path()));

    assert_eq!(analysed.origin(), GraphOrigin::Analysed);
    assert_eq!(cached.origin(), GraphOrigin::Cached);
    assert_eq!(described(&cached), CHARGE_CONNECTED);
    assert_eq!(cached.connections(), analysed.connections());
    assert!(!nowhere.exists());
}

#[test]
fn analysing_a_commit_leaves_exactly_one_graph_in_the_cache_folder() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");

    find_in(&repo, id, temp.path(), &cache_in(cache.path()));
    assert_eq!(
        files_under(cache.path()).len(),
        1,
        "{:?}",
        files_under(cache.path())
    );

    find_in(&repo, id, temp.path(), &cache_in(cache.path()));
    assert_eq!(
        files_under(cache.path()).len(),
        1,
        "{:?}",
        files_under(cache.path())
    );
}

#[test]
fn a_damaged_cache_file_is_analysed_again_and_rebuilt() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");
    find_in(&repo, id, temp.path(), &cache_in(cache.path()));
    let [graph] = files_under(cache.path())
        .try_into()
        .expect("one cached graph");
    fs::write(cache.path().join(graph), b"not a graph").expect("damage the cached graph");

    let rebuilt = find_in(&repo, id, temp.path(), &cache_in(cache.path()));
    let again = find_in(&repo, id, temp.path(), &cache_in(cache.path()));

    assert_eq!(rebuilt.origin(), GraphOrigin::Analysed);
    assert_eq!(described(&rebuilt), CHARGE_CONNECTED);
    assert_eq!(again.origin(), GraphOrigin::Cached);
    assert_eq!(described(&again), CHARGE_CONNECTED);
}

#[test]
fn a_graph_cached_by_another_engine_version_or_in_another_cache_format_is_never_used() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");
    let elsewhere = TempDir::new().expect("create another cache folder");
    // Another tree's graph, which has no connected code for the change.
    let stale = find(
        &Memory::default().with("src/main.rs", FileKind::Blob, "fn main() {}\n"),
        &Change::new(String::new(), Vec::new(), String::new(), Vec::new()),
        &ExcludePatterns::default(),
        &AnalysisDirs::new(temp.path().to_path_buf()),
        &cache_in(elsewhere.path()),
    )
    .expect("find connected code")
    .graph()
    .clone();
    let tree = SnapshotId::Tree(
        repo.open()
            .handle()
            .with_tree(id, |tree: &CommitTree<'_>| tree.id())
            .expect("read the commit's tree"),
    );
    let graphs = cache_in(cache.path());
    graphs.put(&cache::GraphKey::of(tree, "0.10.0"), &stale);
    graphs.put(
        &cache::GraphKey::of(tree, analysis::ENGINE_VERSION).in_another_format(),
        &stale,
    );

    let found = find_in(&repo, id, temp.path(), &graphs);

    assert_eq!(found.origin(), GraphOrigin::Analysed);
    assert_eq!(described(&found), CHARGE_CONNECTED);

    // Cached by this version in this format, the same graph is used.
    graphs.put(&cache::GraphKey::of(tree, analysis::ENGINE_VERSION), &stale);
    let planted = find_in(&repo, id, temp.path(), &graphs);
    assert_eq!(planted.origin(), GraphOrigin::Cached);
    assert_eq!(described(&planted), Vec::<String>::new());
}

#[test]
fn graphs_are_cached_under_the_engine_version_cargo_pins() {
    let manifest = include_str!("../../Cargo.toml");

    let pin = format!("\ncgg = \"={}\"\n", analysis::ENGINE_VERSION);
    assert!(manifest.contains(&pin), "{pin:?} is not in Cargo.toml");
}

#[test]
fn a_cache_folder_that_cannot_be_written_still_gives_connected_code() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    // A file where the cache folder should be, which stops even a
    // privileged user from writing there.
    let folder = temp.path().join("cache");
    fs::write(&folder, "").expect("write a file where the cache folder goes");

    let first = find_in(&repo, id, temp.path(), &cache_in(&folder));
    let second = find_in(&repo, id, temp.path(), &cache_in(&folder));

    for found in [first, second] {
        assert_eq!(found.origin(), GraphOrigin::Analysed);
        assert_eq!(described(&found), CHARGE_CONNECTED);
    }
}

#[test]
fn a_python_change_connects_to_the_callers_and_callees_of_the_method_it_modifies() {
    let repo = TestRepo::new();
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
    let id = repo.commit("Double the charge");

    assert_eq!(
        connected(&repo, id),
        [
            "shop/cart.py 4-6 high: calls charge in shop/pay.py",
            "shop/check.py 1-2 high: called by charge in shop/pay.py",
        ]
    );
}

/// A Python module of three functions, each called from its own file, with
/// `sub` recording its result through `records.record`.
fn python_ops() -> TestRepo {
    let repo = TestRepo::new();
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

/// What finding connected code gives for a change to `sub` alone.
const SUB_CONNECTED: [&str; 2] = [
    "records.py 1-2 high: called by sub in ops.py",
    "use_sub.py 4-5 high: calls sub in ops.py",
];

#[test]
fn a_change_inside_one_function_of_several_connects_only_that_function() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"));
    let id = repo.commit("Subtract the other way round");

    assert_eq!(connected(&repo, id), SUB_CONNECTED);
}

#[test]
fn removing_lines_inside_a_function_changes_that_function() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("    total = round(total)\n", ""));
    let id = repo.commit("Keep the fraction");

    assert_eq!(connected(&repo, id), SUB_CONNECTED);
}

#[test]
fn removing_the_last_lines_of_a_python_function_changes_that_function() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("    return total\n", ""));
    let id = repo.commit("Return nothing");

    assert_eq!(connected(&repo, id), SUB_CONNECTED);
}

#[test]
fn deleting_a_function_finds_no_connected_code_for_it_or_its_neighbours() {
    let repo = python_ops();
    let without_sub = OPS.replace(
        "def sub(a, b):\n    total = a - b\n    total = round(total)\n    record(total)\n    return total\n\n\n",
        "",
    );
    repo.write("ops.py", &without_sub);
    let first = repo.commit("Drop sub");
    repo.write(
        "ops.py",
        without_sub.replace("\n\ndef mul(a, b):\n    return a * b\n", ""),
    );
    let last = repo.commit("Drop mul");

    assert_eq!(connected(&repo, first), Vec::<String>::new());
    assert_eq!(connected(&repo, last), Vec::<String>::new());
}

#[test]
fn a_caller_in_a_file_the_change_touches_is_not_connected_code() {
    let repo = python_ops();
    repo.write("ops.py", OPS.replace("total = a - b", "total = b - a"))
        .write(
            "use_sub.py",
            "from ops import sub\n\n\ndef none_left(a):\n    return sub(a, a) + 0\n",
        );
    let id = repo.commit("Subtract the other way round, and say so");

    assert_eq!(
        connected(&repo, id),
        ["records.py 1-2 high: called by sub in ops.py"]
    );
}

#[test]
fn callers_that_overlap_or_touch_in_one_file_merge_into_one_range_with_all_relations() {
    let repo = python_ops();
    repo.write(
        "callers.py",
        "from ops import add, sub\n\
         \n\
         \n\
         def first(a):\n\
         \x20   return add(a, 1)\n\
         def second(a):\n\
         \x20   return sub(a, 1)\n\
         \n\
         \n\
         def outer(a):\n\
         \x20   def inner(b):\n\
         \x20       return sub(b, 1)\n\
         \x20   return sub(inner(a), 2)\n\
         \n\
         \n\
         def apart(a):\n\
         \x20   return sub(a, 2)\n",
    );
    repo.commit("Call the operations");
    repo.write(
        "ops.py",
        OPS.replace("a + b", "b + a")
            .replace("total = a - b", "total = b - a"),
    );
    let id = repo.commit("Swap the operands");

    let connected = connected(&repo, id);

    let in_callers: Vec<_> = connected
        .iter()
        .filter(|connection| connection.starts_with("callers.py"))
        .collect();
    assert_eq!(
        in_callers,
        [
            "callers.py 4-7 high: calls add in ops.py, calls sub in ops.py",
            "callers.py 10-13 high: calls sub in ops.py",
            "callers.py 16-17 high: calls sub in ops.py",
        ]
    );
}

#[test]
fn calls_of_low_confidence_are_not_connected_code() {
    let repo = TestRepo::new();
    let server = |test: &str| {
        format!(
            "class BillingServicer:\n    def Charge(self, request, context):\n        return {test}\n"
        )
    };
    // The engine links the `Charge` rpc to the servicer's `Charge` method
    // by name alone, at low confidence.
    repo.write(
        "billing.proto",
        "syntax = \"proto3\";\n\
         \n\
         package billing;\n\
         \n\
         service Billing {\n\
         \x20 rpc Charge (ChargeRequest) returns (ChargeReply);\n\
         }\n\
         \n\
         message ChargeRequest {\n\
         \x20 int32 amount = 1;\n\
         }\n\
         \n\
         message ChargeReply {\n\
         \x20 bool ok = 1;\n\
         }\n",
    )
    .write("server.py", server("request.amount > 0"))
    .write(
        "client.py",
        "from server import BillingServicer\n\
         \n\
         \n\
         def pay():\n\
         \x20   return BillingServicer().Charge(None, None)\n",
    );
    repo.commit("Serve billing");
    repo.write("server.py", server("request.amount >= 0"));
    let id = repo.commit("Allow free charges");

    assert_eq!(
        connected(&repo, id),
        ["client.py 4-5 high: calls Charge in server.py"]
    );
}

#[test]
fn a_callable_the_engine_makes_up_is_connected_code_only_if_it_has_lines_of_its_own() {
    let repo = TestRepo::new();
    let billing = |test: &str| {
        format!(
            "function charge(amount) {{\n  return {test};\n}}\n\nmodule.exports = {{ charge }};\n"
        )
    };
    // The engine makes up a synthetic callable for the route's inline
    // handler, on its lines, and for the framework that calls it, on none.
    repo.write("billing.js", billing("amount > 0")).write(
        "routes.js",
        "const express = require('express');\n\
         const { charge } = require('./billing');\n\
         \n\
         const app = express();\n\
         \n\
         app.post('/charge', (req, res) => {\n\
         \x20 res.json(charge(req.body.amount));\n\
         });\n\
         \n\
         function direct(amount) {\n\
         \x20 return charge(amount);\n\
         }\n",
    );
    repo.commit("Charge over HTTP");
    repo.write("billing.js", billing("amount >= 0"));
    let id = repo.commit("Allow free charges");

    assert_eq!(
        connected(&repo, id),
        [
            "routes.js 6-8 high: calls charge in billing.js",
            "routes.js 10-12 high: calls charge in billing.js",
        ]
    );
}

#[test]
fn the_whole_graph_is_kept_and_never_shows_names_when_debugged() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");

    let (change, found) = found_in(&repo, id, temp.path(), &cache_in(cache.path()));

    assert_eq!(found.graph().connections(&change), found.connections());
    let debug = format!("{:?}", found.graph());
    assert_eq!(debug, "Graph { files: 4, callables: 3, edges: 2 }");
}

#[test]
fn the_log_records_counts_sizes_and_timings_but_never_files_callables_or_contents() {
    let (repo, id) = double_the_charge();
    let temp = TempDir::new().expect("create a temporary directory");
    let cache = TempDir::new().expect("create a cache folder");
    let log = Arc::new(Mutex::new(Vec::new()));
    let writer = Arc::clone(&log);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || Captured(Arc::clone(&writer)))
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        find_in(&repo, id, temp.path(), &cache_in(cache.path()));
        find_in(&repo, id, temp.path(), &cache_in(cache.path()));
    });

    let log = String::from_utf8(log.lock().expect("log lock").clone()).expect("UTF-8");
    for logged in [
        "wrote a commit's files for analysis",
        "written=4",
        "cached a graph",
        "origin=Analysed",
        "loaded a cached graph",
        "origin=Cached",
        "found connected code",
        "connections=2",
    ] {
        assert!(log.contains(logged), "{logged} not in {log}");
    }
    for name in [
        "charge", "validate", "checkout", "Payments", "pay.rs", "amount", "graphs", ".json",
    ] {
        assert!(!log.contains(name), "{name} in {log}");
    }
}

/// A writer that appends to a shared buffer.
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

#[test]
fn only_files_the_engine_reads_are_written_and_never_excluded_binary_or_secret_ones() {
    let snapshot = Memory::default()
        .with("src/main.rs", FileKind::Blob, "fn main() {}\n")
        .with(
            "scripts/release",
            FileKind::Executable,
            "#!/usr/bin/env python3\nprint('released')\n",
        )
        .with(
            "api/openapi.yaml",
            FileKind::Blob,
            "openapi: 3.0.0\npaths: {}\n",
        )
        .with("include/shape.h", FileKind::Blob, "int area(void);\n")
        .with("config/settings.yaml", FileKind::Blob, "debug: true\n")
        .with("README.md", FileKind::Blob, "# Shapes\n")
        .with(
            "config/credentials.py",
            FileKind::Blob,
            "TOKEN = 'hunter2'\n",
        )
        .with("deploy/secrets.rb", FileKind::Blob, "TOKEN = 'hunter2'\n")
        .with(".env", FileKind::Blob, "TOKEN=hunter2\n")
        .with("Cargo.lock", FileKind::Blob, "version = 4\n")
        .with("fixtures/huge.rs", FileKind::Blob, "fn fixture() {}\n")
        .with("src/blob.rs", FileKind::Blob, b"fn \0main() {}\n".to_vec())
        .with("src/latin1.py", FileKind::Blob, b"caf\xe9 = 1\n".to_vec())
        .with("src/link.rs", FileKind::Symlink, "main.rs")
        .with("vendor/shapes", FileKind::Submodule, "")
        .with("target/debug/build.rs", FileKind::Blob, "fn main() {}\n")
        .with(
            "web/node_modules/left/pad.js",
            FileKind::Blob,
            "function pad() {}\n",
        )
        .with("src/bin/tool.rs", FileKind::Blob, "fn main() {}\n")
        .with("../outside.rs", FileKind::Blob, "fn main() {}\n")
        .with("src/./here.rs", FileKind::Blob, "fn main() {}\n");
    let exclude = ExcludePatterns::parse(["fixtures/"]).expect("a valid pattern");
    let temp = TempDir::new().expect("create a temporary directory");

    let dir = AnalysisDir::write(
        &snapshot,
        &exclude,
        &AnalysisDirs::new(temp.path().to_path_buf()),
    )
    .expect("write the files");

    assert_eq!(
        files_under(dir.path()),
        [
            "api/openapi.yaml",
            "include/shape.h",
            "scripts/release",
            "src/main.rs"
        ]
    );
    let name = dir
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a UTF-8 name");
    assert!(name.starts_with("whetstone-analysis-"), "{name}");
    assert_eq!(dir.path().parent(), Some(temp.path()));
    let mode = fs::metadata(dir.path())
        .expect("the directory's metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn every_file_of_a_commit_larger_than_the_writers_queue_is_written() {
    let snapshot = (0..1000).fold(Memory::default(), |snapshot, n| {
        snapshot.with(
            &format!("src/m{}/f{n}.rs", n % 37),
            FileKind::Blob,
            format!("pub fn f{n}() {{}}\n"),
        )
    });
    let temp = TempDir::new().expect("create a temporary directory");

    let dir = AnalysisDir::write(
        &snapshot,
        &ExcludePatterns::default(),
        &AnalysisDirs::new(temp.path().to_path_buf()),
    )
    .expect("write the files");

    let written = files_under(dir.path());
    assert_eq!(written.len(), 1000);
    let contents = fs::read_to_string(dir.path().join("src/m5/f42.rs")).expect("read a file");
    assert_eq!(contents, "pub fn f42() {}\n");
}

#[test]
fn a_file_is_written_exactly_when_the_engines_own_detector_recognises_it() {
    let samples: [(&str, &[u8]); 16] = [
        ("lib.rs", b"fn f() {}\n"),
        ("Shape.C", b"int f() { return 0; }\n"),
        ("shape.h", b"int f(void);\n"),
        ("main.GO", b"package main\n"),
        ("tool", b"#!/usr/bin/env python3\nprint(1)\n"),
        ("tool.txt", b"#!/bin/bash\necho 1\n"),
        ("odd", b"#!/usr/bin/env not-a-language\n"),
        ("notes", b"no shebang\n"),
        ("api.yaml", b"# An API\nopenapi: 3.1.0\n"),
        ("api.json", b"{\"swagger\": \"2.0\", \"paths\": {}}"),
        ("events.YML", b"asyncapi: '2.6.0'\n"),
        ("settings.yaml", b"debug: true\n"),
        ("package.json", b"{\"name\": \"shapes\"}\n"),
        ("README.md", b"# Shapes\n"),
        ("schema.graphql", b"type Query { shape: Int }\n"),
        ("billing.proto", b"syntax = \"proto3\";\n"),
    ];
    let originals = TempDir::new().expect("create a temporary directory");
    let mut snapshot = Memory::default();
    for (path, contents) in samples {
        fs::write(originals.path().join(path), contents).expect("write a sample");
        snapshot = snapshot.with(path, FileKind::Blob, contents);
    }
    let registry = cgg_lang::PluginRegistry::with_v1_plugins();
    let detector = cgg_lang::LanguageDetector::new(&registry);
    let recognised: Vec<String> = samples
        .iter()
        .filter(|(path, _)| {
            detector.detect(&originals.path().join(path)).verdict
                != cgg_lang::DetectVerdict::Unknown
        })
        .map(|(path, _)| (*path).to_owned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let temp = TempDir::new().expect("create a temporary directory");

    let dir = AnalysisDir::write(
        &snapshot,
        &ExcludePatterns::default(),
        &AnalysisDirs::new(temp.path().to_path_buf()),
    )
    .expect("write the files");

    assert_eq!(files_under(dir.path()), recognised);
    assert!(
        recognised.len() > 8 && recognised.len() < samples.len(),
        "{recognised:?}"
    );
}

#[test]
fn the_analysis_directory_is_removed_after_the_analysis_and_after_a_panic() {
    let snapshot = Memory::default().with("src/main.rs", FileKind::Blob, "fn main() {}\n");
    let temp = TempDir::new().expect("create a temporary directory");

    let dir = AnalysisDir::write(
        &snapshot,
        &ExcludePatterns::default(),
        &AnalysisDirs::new(temp.path().to_path_buf()),
    )
    .expect("write the files");
    dir.analyse().expect("analyse the files");
    drop(dir);
    assert_eq!(files_under(temp.path()), Vec::<String>::new());

    let interrupted = panic::catch_unwind(AssertUnwindSafe(|| {
        let dir = AnalysisDir::write(
            &snapshot,
            &ExcludePatterns::default(),
            &AnalysisDirs::new(temp.path().to_path_buf()),
        )
        .expect("write the files");
        assert_eq!(files_under(dir.path()), ["src/main.rs"]);
        panic!("the analysis was interrupted");
    }));

    assert!(interrupted.is_err());
    assert_eq!(files_under(temp.path()), Vec::<String>::new());
}

/// A snapshot that runs `on_read` before it reads any file, as something
/// can happen while a job writes a commit's files.
struct Interrupted<'a, F: Fn()> {
    snapshot: &'a Memory,
    on_read: F,
}

impl<F: Fn()> fmt::Debug for Interrupted<'_, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interrupted").finish_non_exhaustive()
    }
}

impl<F: Fn()> Snapshot for Interrupted<'_, F> {
    type Error = Infallible;

    fn id(&self) -> SnapshotId {
        self.snapshot.id()
    }

    fn files(&self) -> Result<Vec<SnapshotFile>, Infallible> {
        self.snapshot.files()
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, Infallible> {
        (self.on_read)();
        self.snapshot.read(path)
    }
}

/// A snapshot of two source files.
fn two_files() -> Memory {
    Memory::default()
        .with("src/main.rs", FileKind::Blob, "fn main() {}\n")
        .with("src/lib.rs", FileKind::Blob, "pub fn lib() {}\n")
}

#[test]
fn quitting_removes_an_analysis_directory_in_use_and_stops_any_more_from_being_written() {
    let temp = TempDir::new().expect("create a temporary directory");
    let dirs = AnalysisDirs::new(temp.path().to_path_buf());
    let guard = dirs.guard();
    let snapshot = two_files();

    let dir =
        AnalysisDir::write(&snapshot, &ExcludePatterns::default(), &dirs).expect("write the files");
    assert_eq!(files_under(dir.path()), ["src/lib.rs", "src/main.rs"]);
    drop(guard);

    assert_eq!(files_under(temp.path()), Vec::<String>::new());
    let change = Change::new(String::new(), Vec::new(), String::new(), Vec::new());
    let cache = TempDir::new().expect("create a cache folder");
    let failure = find(
        &snapshot,
        &change,
        &ExcludePatterns::default(),
        &dirs,
        &cache_in(cache.path()),
    )
    .expect_err("nothing is written once the app quits");
    assert!(matches!(failure, Failure::Quitting), "{failure:?}");
    drop(dir);
    assert_eq!(files_under(temp.path()), Vec::<String>::new());
}

#[test]
fn quitting_while_a_commits_files_are_written_stops_before_the_next_and_leaves_nothing() {
    let temp = TempDir::new().expect("create a temporary directory");
    let dirs = AnalysisDirs::new(temp.path().to_path_buf());
    let guard = std::cell::RefCell::new(Some(dirs.guard()));
    let snapshot = two_files();
    let written = std::cell::Cell::new(0);
    let quitting = Interrupted {
        snapshot: &snapshot,
        // Quit as the second file is read.
        on_read: || {
            written.set(written.get() + 1);
            if written.get() == 2 {
                guard.borrow_mut().take();
            }
        },
    };
    let change = Change::new(String::new(), Vec::new(), String::new(), Vec::new());
    let cache = TempDir::new().expect("create a cache folder");

    let failure = find(
        &quitting,
        &change,
        &ExcludePatterns::default(),
        &dirs,
        &cache_in(cache.path()),
    )
    .expect_err("the app quit");

    assert!(matches!(failure, Failure::Quitting), "{failure:?}");
    assert_eq!(files_under(temp.path()), Vec::<String>::new());
}

#[test]
fn a_panic_while_finding_connected_code_is_a_failure_and_leaves_nothing() {
    let temp = TempDir::new().expect("create a temporary directory");
    let snapshot = two_files();
    let panicking = Interrupted {
        snapshot: &snapshot,
        on_read: || panic!("a file cannot be read"),
    };
    let change = Change::new(String::new(), Vec::new(), String::new(), Vec::new());
    let cache = TempDir::new().expect("create a cache folder");

    let failure = find(
        &panicking,
        &change,
        &ExcludePatterns::default(),
        &AnalysisDirs::new(temp.path().to_path_buf()),
        &cache_in(cache.path()),
    )
    .expect_err("finding connected code panicked");

    assert!(matches!(failure, Failure::Panicked), "{failure:?}");
    assert_eq!(files_under(temp.path()), Vec::<String>::new());
}

#[test]
fn a_temporary_directory_inside_a_directory_the_engine_never_reads_fails_before_writing() {
    let temp = TempDir::new().expect("create a temporary directory");
    let build = temp.path().join("build");
    fs::create_dir(&build).expect("create a build directory");
    let snapshot = Memory::default().with("src/main.rs", FileKind::Blob, "fn main() {}\n");
    let change = Change::new(String::new(), Vec::new(), String::new(), Vec::new());
    let cache = TempDir::new().expect("create a cache folder");

    let failure = find(
        &snapshot,
        &change,
        &ExcludePatterns::default(),
        &AnalysisDirs::new(build),
        &cache_in(cache.path()),
    )
    .expect_err("the engine would read nothing there");

    assert!(
        matches!(failure, Failure::UnreadableTemporaryDirectory("build")),
        "{failure:?}"
    );
    assert_eq!(files_under(temp.path()), Vec::<String>::new());
}

#[test]
fn the_sweep_removes_only_analysis_directories_unchanged_for_more_than_a_day() {
    let temp = TempDir::new().expect("create a temporary directory");
    let now = SystemTime::now();
    let made = |name: &str, age: Duration| {
        let path = temp.path().join(name);
        fs::create_dir_all(path.join("src")).expect("create a directory");
        fs::write(path.join("src/main.rs"), "fn main() {}\n").expect("write a file");
        fs::File::open(&path)
            .and_then(|dir| dir.set_modified(now - age))
            .expect("date the directory");
    };
    made("whetstone-analysis-old", Duration::from_hours(25));
    made("whetstone-analysis-recent", Duration::from_hours(23));
    made("other-old", Duration::from_hours(48));
    made("linked-old", Duration::from_hours(48));
    std::os::unix::fs::symlink(
        temp.path().join("linked-old"),
        temp.path().join("whetstone-analysis-link"),
    )
    .expect("make a symbolic link");
    fs::write(temp.path().join("whetstone-analysis-file"), "").expect("write a file");

    sweep(temp.path(), now);

    let mut left: Vec<String> = fs::read_dir(temp.path())
        .expect("read the temporary directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "linked-old",
            "other-old",
            "whetstone-analysis-file",
            "whetstone-analysis-link",
            "whetstone-analysis-recent",
        ]
    );
    assert!(temp.path().join("linked-old/src/main.rs").exists());
}

#[test]
fn calls_of_medium_confidence_are_connected_code_in_the_medium_tier() {
    let repo = TestRepo::new();
    let billing = |amount: &str| {
        format!("class Billing:\n    def charge(self, amount):\n        return {amount}\n")
    };
    // Nothing says what `processor` is, so the engine links its `charge`
    // to the only one there is, at medium confidence.
    repo.write("billing.py", billing("amount")).write(
        "pay.py",
        "def pay(processor):\n    return processor.charge(1)\n",
    );
    repo.commit("Start billing");
    repo.write("billing.py", billing("amount * 2"));
    let id = repo.commit("Double the charge");

    assert_eq!(
        connected(&repo, id),
        ["pay.py 1-2 medium: calls charge in billing.py"]
    );
}
