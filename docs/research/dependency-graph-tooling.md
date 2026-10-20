# Dependency graph tooling for a change

Research note, 19 October 2026. **Status: input for the decision on [#9 "Choose how to build a change's dependency graph"](https://github.com/monadial/whetstone/issues/9), not a decision.** The decision was made on 20 October 2026 in [#28 "Choose the dependency-graph engine and its binary-size cost"](https://github.com/monadial/whetstone/issues/28) and is recorded in [ADR 0002](../adr/0002-embed-cgg-for-connected-code.md).

**Question.** For a change (one commit) in any Git repository, how should Whetstone build a one-hop dependency graph locally, with ordinary code and no AI? The graph covers what the changed code calls, what calls it, and the related tests and config. Whetstone accepts repositories in any language, including repositories that are not code. The graph is best effort: a file without graph support still contributes its diff and its full content to the evidence (#9).

The user asked specifically whether CodeGraph or one of its alternatives ships as a Rust crate that embeds directly in the CLI. This note also compares tree-sitter approaches (tags queries, stack graphs), `syn`, rust-analyzer's crates, SCIP and LSP.

**Method.**
- Every fact was checked on 2026-10-19 against a primary source: crates.io API data, docs.rs, the projects' repositories (README, LICENSE, Cargo.toml, source), their own docs, and the original research papers.
- **Versions and dates** come from the crates.io API (`https://crates.io/api/v1/crates/<name>`). **Repo health** comes from the GitHub API (`archived`, `pushed_at`, releases).
- **Measurements** were taken on this machine, with probe binaries built in a scratch directory. The probes are not committed.
  - Machine: Apple M4 Max, macOS, rustc 1.93.1.
  - Release profile: `opt-level = 3`, `lto = true`, `codegen-units = 1`, `strip = true`.
  - Main corpus: the source of the published `tokio` 1.53.3 crate ([crates.io](https://crates.io/crates/tokio/1.53.3)): 556 `.rs` files, 4.5 MB, 141,442 lines.
  - All timings are warm-cache wall clock, reported as the range over two or three runs. Single-threaded unless stated.
- **Markers:** *(measured)* means my own measurement. *(inference)* means my own reasoning from the sources. **UNCONFIRMED** means no primary source was found.
- **Layout:** each section separates **Facts** from **Proposal**.

## Summary

- **CodeGraph does not ship as a Rust crate.**
  - It is a TypeScript/Node application ([repo](https://github.com/colbymchenry/codegraph)).
  - Its Rust "kernel" is an unpublished N-API `cdylib` (`publish = false`) that only extracts single files ([Cargo.toml](https://github.com/colbymchenry/codegraph/blob/main/codegraph-kernel/Cargo.toml)).
  - All resolution happens in TypeScript ([resolution/index.ts](https://github.com/colbymchenry/codegraph/blob/main/src/resolution/index.ts)).
- **None of the alternatives on the tokenade.net page embeds as a crate today.**
  - [Tokenade](https://github.com/pi-infected/tokenade) is Rust but is not on crates.io.
  - serena is GPL-3.0-or-later and mcp-codebase-index is AGPL-3.0.
  - claude-context needs a vector database and an OpenAI key.
  - The rest are Python or TypeScript (section 1).
- **Two Rust crates outside that list are libraries built for this job.** Both are permissive, offline and young, with one maintainer each:
  - **`cgg`** 0.11.0: Apache-2.0 OR MIT, 46 languages, scope-aware resolution and a confidence level on every edge ([README](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md)).
  - **`code2graph`** 0.0.0-beta.17: Apache-2.0, grammars as Cargo features ([README](https://github.com/NodeDB-Lab/code2graph/blob/HEAD/README.md)).
- **cgg embeds and works, but it is big** *(measured)*:
  - A one-hop query on tokio matched the source exactly: one caller and two callees, with the right one of two `from_parts` methods picked.
  - Analysing all of tokio takes 0.29–0.34 s with its default thread pool and 1.29 s on one thread.
  - The binary is **115 MB stripped (11.7 MB gzipped)**, because about 45 grammars are compiled in with no feature gating.
  - It pins tree-sitter 0.26.
- **Building our own on tree-sitter is small but shallow** *(measured)*:
  - Rust alone adds 2.4 MB; 16 grammar crates add 21.8 MB.
  - Tags extraction over tokio takes 0.44–0.47 s.
  - The tags queries the grammars ship are uneven:
    - C, C++ and Swift tag no calls.
    - TypeScript gets calls only when combined with the JavaScript query.
    - C#'s query fails to load in `tree-sitter-tags`.
    - Rust's query misses calls of the form `Type::f()`, which add 28% more call references on tokio once tagged.
- **`syn` is Rust-only and fails a whole file on one syntax error.** It adds 0.7 MB and parses tokio in 0.21–0.22 s *(measured)*.
- **rust-analyzer gives resolved edges but is not embeddable in practice.**
  - The `ide` API is "explicitly unstable", releases are weekly, and the MSRV is 1.98 ([architecture](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)).
  - It needs `cargo metadata` and `rust-src` to load a project.
  - The server binary is 36–40 MiB.
- **GitHub's `stack-graphs` is archived.** The archived crates pin tree-sitter 0.24, so they cannot be linked next to a newer tree-sitter ([repo](https://github.com/github/stack-graphs)).
- **Non-code repositories can still get a useful graph from cheap, language-agnostic signals:**
  - Markdown links through `pulldown-cmark`, which adds 0.26 MB and reads 1,344 files in 66 ms *(measured)*.
  - Path mentions.
  - Co-change mined from Git history: Zimmermann et al. report 26% of further files predicted, and a correct location in the top three 64% of the time ([ICSE 2004](https://thomas-zimmermann.com/publications/files/zimmermann-icse-2004.pdf)).
  - Test naming conventions.
- **Proposal:**
  - Put the graph behind a Whetstone-owned module.
  - Embed `cgg` as the first code-edge engine.
  - Add Whetstone's own providers for links, path mentions, tests, config and co-change.
  - **Main trade-off:** accuracy and 46 languages now, against a binary roughly 110 MB larger and a young, single-maintainer dependency.
  - **Fallback:** our own tree-sitter layer for a short list of languages.

## Options at a glance

Sizes are the stripped release-binary growth over a 312 KiB hello-world, measured with the profile above. Speeds are for tokio 1.53.3.

| Option | Edge accuracy | Speed (tokio) | Binary growth | Languages | Licence | Maintenance | Embeds as crate? |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **cgg** 0.11.0 | Resolved by scope, imports, receiver type; confidence per edge; 14,599 edges + 14,858 unresolved calls *(measured)* | 0.29–0.34 s (default pool), 1.29 s (1 thread) *(measured)* | +114.8 MB (11.7 MB gz) *(measured)* | 46 + descriptor formats; no Markdown | Apache-2.0 OR MIT | Active; 20 versions since 2026-08-11; one main author | Yes (`cgg::analyze`) |
| **code2graph** beta.17 | Tiered; 87% of tokio edges are name-only or heuristic *(measured)* | ≈9.0 s (1 thread) *(measured)* | +2.7 MB with Rust only *(measured)* | ≈22, feature-gated; no Markdown | Apache-2.0 | Pre-0.1; one maintainer | Yes |
| Own: `tree-sitter` + `tree-sitter-tags` + grammars | Name-based; shipped queries uneven | 0.44–0.47 s (Rust, 1 thread) *(measured)* | +2.4 MB (Rust); +21.8 MB (16 grammar crates) *(measured)* | Any grammar we add | MIT (core and the grammars checked) | Core very active | Yes (we write the graph) |
| `syn` 3 | Name/path-based; macro bodies opaque; one error fails the file | 0.21–0.22 s (1 thread) *(measured)* | +0.7 MB *(measured)* | Rust only | MIT OR Apache-2.0 | Very active | Yes |
| rust-analyzer `ra_ap_*` | Resolved, type-inferred | 5.8 s / 246 MB on ripgrep in CI | Full server 36–40 MiB | Rust only | MIT OR Apache-2.0 | Weekly, unstable API, MSRV 1.98 | Technically; costly |
| SCIP via external indexers | Compiler-grade, whole project | Full build/index per language | Small reader crate | Many, each needs its toolchain | Apache-2.0 reader | Active | Reader only |
| LSP to installed servers | Resolved where servers support call hierarchy | Server cold start + project load | Small client | Any with a server | Client crates permissive | Spec stable | Client only |
| stack-graphs | Was precise for 4 languages | n/a | n/a | Java, JS, Python, TS | MIT OR Apache-2.0 | **Archived** | Conflicts with tree-sitter ≥ 0.25 |

## 1. CodeGraph and its alternatives

### Facts

**CodeGraph** ([colbymchenry/codegraph](https://github.com/colbymchenry/codegraph)). This is the CodeGraph indexed in this repo's `.codegraph/` directory; npm `@colbymchenry/codegraph` 1.6.0 is installed locally.
- **What it is.**
  - TypeScript application, MIT ([LICENSE](https://github.com/colbymchenry/codegraph/blob/main/LICENSE)).
  - The npm package is a shim that installs a per-platform bundle carrying its own Node runtime ([BUNDLING.md](https://github.com/colbymchenry/codegraph/blob/main/BUNDLING.md)). The v1.6.2 bundles are 54–63 MB compressed ([releases](https://api.github.com/repos/colbymchenry/codegraph/releases)).
  - Telemetry is on by default and anonymous, with an opt-out ([TELEMETRY.md](https://github.com/colbymchenry/codegraph/blob/main/TELEMETRY.md)).
- **How it builds edges.**
  - Tree-sitter extraction produces nodes and edges (calls, imports, extends, implements). Resolution runs in TypeScript in this order ([resolution/index.ts](https://github.com/colbymchenry/codegraph/blob/main/src/resolution/index.ts)):
    1. framework resolvers;
    2. import-based resolution;
    3. confidence-scored name matching ([name-matcher.ts](https://github.com/colbymchenry/codegraph/blob/main/src/resolution/name-matcher.ts)).
  - Storage is SQLite with FTS5 ([schema.sql](https://github.com/colbymchenry/codegraph/blob/main/src/db/schema.sql)).
  - It covers about 34 languages ([README](https://github.com/colbymchenry/codegraph/blob/main/README.md)).
  - It does not index config or docs. Its own agent instructions say to use Read/Grep "for what codegraph doesn't index (configs, docs)" ([server-instructions.ts](https://github.com/colbymchenry/codegraph/blob/main/src/mcp/server-instructions.ts)).
- **As a Rust library: no.**
  - `codegraph-kernel` is `publish = false`, `crate-type = ["cdylib"]` ([Cargo.toml](https://github.com/colbymchenry/codegraph/blob/main/codegraph-kernel/Cargo.toml)).
  - It exports only `#[napi]` per-file extraction functions; "everything downstream (resolution…) is untouched" ([lib.rs](https://github.com/colbymchenry/codegraph/blob/main/codegraph-kernel/src/lib.rs)).
  - Local inspection of the installed 1.6.0 kernel shows tree-sitter 0.25.10, napi 3.10.5 and regex linked in *(measured)*.
- **Maintenance.**
  - Very active: 33 GitHub releases since 2026-01-18, last push 2026-10-19 ([API](https://api.github.com/repos/colbymchenry/codegraph)).
  - In practice one maintainer: 1,378 commits from the owner, and the next contributor is a bot ([contributors](https://api.github.com/repos/colbymchenry/codegraph/contributors)).
- **The `codegraph` name on crates.io** belongs to an unrelated RocksDB-backed graph library (Apache-2.0, 0.2.0, 2026-02-15) whose listed repository returns 404 ([crate](https://crates.io/api/v1/crates/codegraph), [repo API](https://api.github.com/repos/anvanster/codegraph)).

**The alternatives on [tokenade.net](https://tokenade.net/en/comparisons/alternatives/codegraph-alternatives).** The page was updated 2026-10-15 and is written by Tokenade's founder. It links only to Tokenade's repository; the others were found through their package registries.

| Tool | Language | Licence | How it gets edges | Embeds as Rust crate? |
| --- | --- | --- | --- | --- |
| [serena](https://github.com/oraios/serena) | Python | **GPL-3.0-or-later**; SolidLSP part MIT ([LICENSE](https://github.com/oraios/serena/blob/HEAD/LICENSE)) | Installed LSP servers | No (licence, language) |
| [Tokenade](https://github.com/pi-infected/tokenade) | Rust | MIT OR Apache-2.0 | Tree-sitter names + module scope; ambiguous names dropped ([calls.rs](https://github.com/pi-infected/tokenade/blob/HEAD/crates/tokenade-core/src/index/calls.rs)) | No: not on crates.io ([404](https://crates.io/api/v1/crates/tokenade-core)); repo created 2026-10-18 |
| [tokensave](https://github.com/aovestdipaperino/tokensave) | Rust | MIT | Tree-sitter + name/import heuristics ([resolver.rs](https://github.com/aovestdipaperino/tokensave/blob/HEAD/src/resolution/resolver.rs)) | Has a lib target, but docs.rs build fails ([status](https://docs.rs/crate/tokensave/7.15.0/status.json)); pulls libsql, ort, gix; posts to a remote counter ([cloud.rs](https://github.com/aovestdipaperino/tokensave/blob/HEAD/src/cloud.rs)) |
| [claude-context](https://github.com/zilliztech/claude-context) | TypeScript | MIT | Vector search; **needs Milvus/Zilliz and `OPENAI_API_KEY`** ([README](https://github.com/zilliztech/claude-context/blob/HEAD/README.md)) | No (AI, service) |
| [mcp-codebase-index](https://github.com/MikeRecognex/mcp-codebase-index) | Python | **AGPL-3.0** ([LICENSE](https://github.com/MikeRecognex/mcp-codebase-index/blob/HEAD/LICENSE)) | Python `ast`, regex for others | No |
| [codesight](https://github.com/Houseofmvps/codesight) | TypeScript | MIT | TS AST, regex elsewhere; file-level import graph ([README](https://github.com/Houseofmvps/codesight/blob/HEAD/README.md)) | No |
| [code-review-graph](https://github.com/tirth8205/code-review-graph) | Python | MIT | Tree-sitter graph with call, inheritance and **test-coverage** edges; blast radius from Git diffs; Jedi for Python ([README](https://github.com/tirth8205/code-review-graph/blob/HEAD/README.md)) | No; but the closest design reference *(inference)* |
| [graphify](https://github.com/Graphify-Labs/graphify) | Python | Apache-2.0 | Tree-sitter for code; **docs and PDFs need an LLM** ([README](https://github.com/Graphify-Labs/graphify/blob/HEAD/README.md)) | No |

**Rust crates found outside the list.** All are on crates.io (versions from the crates.io API).

- **`cgg`** ([repo](https://github.com/NeuralNotwerk/cgg), [crate](https://crates.io/crates/cgg)): 0.11.0, released 2026-10-16. Apache-2.0 OR MIT, MSRV 1.85.
  - **Library API.**
    - `cgg::analyze(&RunOptions) -> Result<RunOutcome>`. It performs "no I/O beyond reading the source tree", is "safe to call concurrently", and is "pre-1.0 … Pin an exact minor" ([README §Library](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md#library)).
    - The graph exposes callables with file and start/end lines, and edges with `site_line`, `confidence` (`High`/`Medium`/`Low`) and resolver provenance ([graph.rs](https://github.com/NeuralNotwerk/cgg/blob/HEAD/crates/cgg-core/src/graph.rs)).
  - **Resolution** ([README §Resolution pipeline](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md#resolution-pipeline)):
    - type propagation (parameters, locals, constructors, return types);
    - intra-file scope with owner-qualifier disambiguation;
    - cross-file imports, `pub use` and `#include`;
    - FFI and framework entry points.
    - A `--fanout-cap` (default 5) limits same-named candidates for duck-typed method calls.
  - **Other features.**
    - `--since <revspec>` seeds the graph from `git diff`, and `-n 1` gives a one-hop neighbourhood.
    - Fully offline, "no network requests at all".
  - **Limitations it states** ([README §Limitations](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md#limitations)):
    - no generics or fully dynamic typing;
    - no C preprocessor;
    - no on-disk cache (every run re-parses);
    - stack-graphs support removed in the move to tree-sitter 0.26.
  - **Languages.** 46, plus OpenAPI, AsyncAPI, Protobuf, GraphQL and Smithy descriptors. No Markdown.
  - **Dependency weight.**
    - The workspace pins `tree-sitter = "0.26"` ([Cargo.toml](https://github.com/NeuralNotwerk/cgg/blob/HEAD/Cargo.toml)).
    - `cgg-lang` has no feature flags, so every grammar is always compiled ([cgg-lang Cargo.toml](https://github.com/NeuralNotwerk/cgg/blob/HEAD/crates/cgg-lang/Cargo.toml)).
    - The library crate also depends on clap, tracing-subscriber and mimalloc ([cgg Cargo.toml](https://github.com/NeuralNotwerk/cgg/blob/HEAD/crates/cgg/Cargo.toml)).
    - The README states every transitive dependency is permissively licensed ([§License](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md#license)).
  - **Maintenance.** Repository created 2026-05-09. 20 crates.io versions since 0.6.1 on 2026-08-11 ([versions](https://crates.io/api/v1/crates/cgg/versions)). About 261 commits, 232 of them from the owner. 45 stars ([API](https://api.github.com/repos/NeuralNotwerk/cgg)).
  - **Upstream benchmark.** Single-shot timings in the README include ripgrep 196 ms, laravel 467 ms and pandoc 388 ms ([§Benchmark](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md#benchmark)).
- **`code2graph`** ([repo](https://github.com/NodeDB-Lab/code2graph), [docs.rs](https://docs.rs/code2graph/0.0.0-beta.17)): 0.0.0-beta.17, released 2026-07-29. Apache-2.0, MSRV 1.85.
  - It is a "purpose-neutral" library with no storage: `extract_path` produces `FileFacts`, and a `Resolver` turns them into a `CodeGraph`. Each edge has `Confidence` (`Heuristic < NameOnly < Scoped < Exact`) and `Provenance` ([README](https://github.com/NodeDB-Lab/code2graph/blob/HEAD/README.md)).
  - The default stack chains seven resolvers, from symbol table to scope graph to case-folded names ([layered.rs](https://docs.rs/crate/code2graph/0.0.0-beta.17/source/src/resolve/layered.rs)).
  - About 22 grammars are Cargo features. It depends on `tree-sitter = ">=0.24, <0.27"` ([Cargo.toml](https://github.com/NodeDB-Lab/code2graph/blob/HEAD/Cargo.toml)).
  - It is "pre-0.1", Markdown is out of scope, and one maintainer has 401 of 402 commits.
- **`codanna`** (Apache-2.0 with NOTICE), **`narsil-mcp`** (MIT OR Apache-2.0) and **`tokensave`** (MIT) are applications with a library target. Their drawbacks as dependencies:
  - codanna has 60 non-optional dependencies, including fastembed/ONNX ([repo](https://github.com/bartolli/codanna)).
  - narsil-mcp labels its library "for integration tests" and resolves name collisions by first alphabetical match ([callgraph.rs](https://github.com/postrv/narsil-mcp/blob/HEAD/src/callgraph.rs)).
- **`cartog`** (MIT, 0.34.2) is useful mainly as evidence. Its README reports "heuristic-only resolution … lands around 25–37 %" recall on its own 13-scenario benchmark, against 97% with language servers on PATH ([README](https://github.com/jrollin/cartog/blob/HEAD/README.md)).
- **Ruled out by licence:** `magellan` (GPL-3.0-only) ([crate](https://crates.io/api/v1/crates/magellan)).

### Measured: embedding `cgg` 0.11.0 and `code2graph` beta.17

Probe for each: a minimal binary that calls the library and prints edge counts.

- **cgg build and size.**
  - A clean release build took 1 min 39 s. The lockfile has 142 packages, including tree-sitter 0.26.13 and 43 `tree-sitter-*` packages.
  - The binary is **115,071,184 bytes stripped, 11,730,078 bytes gzipped**.
- **cgg on tokio.**
  - 557 files, 6,101 callables.
  - 14,599 edges: 6,851 high, 6,216 medium, 1,532 low confidence.
  - 14,858 unresolved calls.
  - 293–343 ms with the default pool (half the physical cores, capped at 8); 1,289 ms with `jobs = 1`.
- **cgg one-hop spot check.**
  - Function: `Builder::build_current_thread_runtime` in `tokio/src/runtime/builder.rs`.
  - Caller: `Builder::build` (line 1074).
  - Callees: `build_current_thread_runtime_components` (line 1650) and `Runtime::from_parts` (line 1652), all high confidence.
  - Checked by hand against the source: correct and complete. Tokio defines two `from_parts` functions, and cgg picked the right one. The enum-variant constructor on line 1651 was correctly not treated as a call.
  - This is one sample, not an accuracy study.
- **Walker gotcha.** cgg found **0 files** when pointed at tokio inside `~/.cargo/registry`. Its built-in deny list (`node_modules`, `.venv`, `dist`, `.git`, `.cargo`, …) matches *any* path component, ancestors included ([cgg-walk lib.rs](https://github.com/NeuralNotwerk/cgg/blob/HEAD/crates/cgg-walk/src/lib.rs)). The same tree copied elsewhere gave the numbers above.
- **code2graph, Rust feature only.**
  - Clean build 21 s; binary 2,983,456 bytes (927,499 gzipped).
  - On tokio: 556 files, 6,656 symbols.
  - 304,879 edges: 14,674 exact, 24,177 scoped, 168,788 name-only, 97,240 heuristic.
  - 7.6 s to extract and 9.0 s in total, on one thread.
  - *(inference)* The name-only and heuristic tiers dominate, so a consumer has to filter to `Scoped` and above to get useful one-hop edges.

### Proposal

- Treat **cgg** as the only embeddable engine that already does cross-language, scope-aware call resolution with honest confidence labels *(inference)*.
- Treat **code2graph** as the lean alternative to revisit once it reaches 0.1 *(inference)*.
- Use **code-review-graph** as a design reference for test-coverage edges and Git-diff blast radius, not as a dependency (Python) *(inference)*.

## 2. Building directly on tree-sitter

### Facts

**Core runtime.**
- `tree-sitter` 0.27.1 (2026-10-08): MIT, MSRV 1.90 ([crates.io](https://crates.io/crates/tree-sitter)).
- It compiles a C11 runtime through `cc`, so a C compiler is needed at build time ([build.rs](https://github.com/tree-sitter/tree-sitter/blob/v0.27.1/lib/binding_rust/build.rs)).
- It loads grammar ABI 13–15 ([ABI table](https://tree-sitter.github.io/tree-sitter/using-parsers/7-abi-versions.html)).
- It is designed to be "robust enough to provide useful results even in the presence of syntax errors" ([docs](https://tree-sitter.github.io/tree-sitter/)).

**`links` constraint.**
- The crate declares `links = "tree-sitter"`, and Cargo allows at most one package per `links` value ([Cargo docs](https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key)).
- *(inference)* Every crate in the binary that uses tree-sitter must agree on one tree-sitter minor version:
  - cgg is on 0.26;
  - code2graph accepts below 0.27;
  - ast-grep 0.51 needs ^0.27;
  - stack-graphs needs 0.24.

**`tree-sitter-tags`** 0.27.1: MIT, released in lockstep with the core ([crates.io](https://crates.io/crates/tree-sitter-tags)).
- It runs a grammar's `tags.scm` and yields tags with a name, a byte and row span, a kind, and `is_definition` ([tags.rs](https://docs.rs/crate/tree-sitter-tags/0.27.1/source/src/tags.rs)).
- The conventions are `@definition.{function,method,class,interface,module}` and `@reference.{call,class,implementation}` with an inner `@name` ([code navigation docs](https://tree-sitter.github.io/tree-sitter/4-code-navigation.html)).
- It does no scope, import or receiver-type resolution and no cross-file binding *(inference from the `Tag` struct)*.
- GitHub's own code navigation works the same way: it "searches all definitions and references across a repository to find entities with a given name" ([GitHub Docs](https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github)).

**Locals queries** (`@local.scope`, `@local.definition`, `@local.reference`) resolve names within one file ([docs](https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html#local-variables)).
- `tree-sitter-tags` uses them only to suppress patterns marked `#is-not? local` ([tags.rs](https://docs.rs/crate/tree-sitter-tags/0.27.1/source/src/tags.rs)).
- Of the grammar crates checked, only JavaScript, TypeScript, Ruby, Scala and Swift ship a `locals.scm`.

**Shipped tags queries are uneven.** Checked in the published crate sources, and confirmed against each repo's default branch on 2026-10-19:

| Grammar crate | Version | Calls tagged? | Source |
| --- | --- | --- | --- |
| tree-sitter-rust | 0.24.2 | Only `f()`, `x.f()` and `m!()`. **Not** `Type::f()` or `module::f()` | [tags.scm](https://docs.rs/crate/tree-sitter-rust/0.24.2/source/queries/tags.scm) |
| tree-sitter-python | 0.25.0 | Yes | [tags.scm](https://docs.rs/crate/tree-sitter-python/0.25.0/source/queries/tags.scm) |
| tree-sitter-javascript | 0.25.0 | Yes; excludes `require` | [tags.scm](https://docs.rs/crate/tree-sitter-javascript/0.25.0/source/queries/tags.scm) |
| tree-sitter-typescript | 0.23.2 | **No** in its own query; it is meant to be concatenated with the JavaScript query ([tree-sitter.json](https://github.com/tree-sitter/tree-sitter-typescript/blob/v0.23.2/tree-sitter.json)) | [tags.scm](https://docs.rs/crate/tree-sitter-typescript/0.23.2/source/queries/tags.scm) |
| tree-sitter-go | 0.25.0 | Yes | [tags.scm](https://docs.rs/crate/tree-sitter-go/0.25.0/source/queries/tags.scm) |
| tree-sitter-java | 0.23.5 | Yes | [tags.scm](https://docs.rs/crate/tree-sitter-java/0.23.5/source/queries/tags.scm) |
| tree-sitter-c | 0.24.2 | **No references at all** | [tags.scm](https://docs.rs/crate/tree-sitter-c/0.24.2/source/queries/tags.scm) |
| tree-sitter-cpp | 0.23.4 | **No references at all** | [tags.scm](https://docs.rs/crate/tree-sitter-cpp/0.23.4/source/queries/tags.scm) |
| tree-sitter-c-sharp | 0.23.5 | Only `@reference.send`. Line 23 uses `@module`, which `tree-sitter-tags` rejects with `InvalidCapture("module")`, so the query **does not load as shipped** *(measured)* | [tags.scm](https://docs.rs/crate/tree-sitter-c-sharp/0.23.5/source/queries/tags.scm) |
| tree-sitter-ruby | 0.23.1 | Yes | [tags.scm](https://docs.rs/crate/tree-sitter-ruby/0.23.1/source/queries/tags.scm) |
| tree-sitter-php | 0.25.1 | Yes | [tags.scm](https://docs.rs/crate/tree-sitter-php/0.25.1/source/queries/tags.scm) |
| tree-sitter-swift | 0.7.4 | **No references at all** (definitions only) | [tags.scm](https://docs.rs/crate/tree-sitter-swift/0.7.4/source/queries/tags.scm) |
| tree-sitter-bash, -json, -html, -css, -md, -yaml, -toml-ng, kotlin-ng | various | No tags query exported | crates.io pages |

All grammar crates checked are MIT and at ABI 14 or 15. Three cannot be built next to tree-sitter 0.27, because they pin older versions (`links` conflicts):
- fwcd `tree-sitter-kotlin` 0.3.8;
- the old `tree-sitter-toml` 0.20;
- the `parser` feature of `tree-sitter-md`.

**Grammar bundles and loading.**
- **`tree-sitter-language-pack`** 1.22.1 (MIT) covers 372 languages ([repo](https://github.com/xberg-io/tree-sitter-language-pack)).
  - By default it **downloads parsers on first use**; static linking needs build-time environment variables.
  - The licences of its bundled grammars are **UNCONFIRMED** (not listed per grammar).
- **`tree-sitter-loader`** compiles grammar repositories at run time with the user's C compiler ([README](https://github.com/tree-sitter/tree-sitter/blob/v0.27.1/crates/loader/README.md)).
- The **`wasm` feature** embeds Wasmtime with Cranelift. Wasmtime documents its C API at 19 MB for a release build ([docs](https://docs.wasmtime.dev/examples-minimal.html)). The net size added to Whetstone is **UNCONFIRMED**.

**Reference point.** difftastic 0.71.0 embeds about 62 grammars and ships a **118.8 MB** binary, 10.3–12.0 MB compressed ([release](https://github.com/Wilfred/difftastic/releases/tag/0.71.0), [languages](https://github.com/Wilfred/difftastic/blob/0.71.0/manual/src/languages_supported.md)).

**Stack graphs.**
- `github/stack-graphs` is archived: "no longer supported or updated by GitHub". Its final crates came out on 2024-12-13, with rules only for Java, JavaScript, Python and TypeScript ([repo](https://github.com/github/stack-graphs), [crate](https://crates.io/crates/tree-sitter-stack-graphs)).
- `tree-sitter-graph` 0.12.0 last moved on 2024-12-11 ([repo](https://github.com/tree-sitter/tree-sitter-graph)).
- Both pin tree-sitter ^0.24.
- Republished forks exist (`isohypse-sg`, `basemind-stack-graphs`), each with one version; whether either is maintained is **UNCONFIRMED** ([isohypse-sg](https://crates.io/crates/isohypse-sg), [basemind-stack-graphs](https://crates.io/crates/basemind-stack-graphs)).
- GitHub's current docs describe only name-based tree-sitter navigation ([GitHub Docs](https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github)).

**Other tree-sitter libraries.**
- `ast-grep-core` 0.51.0 (MIT) does pattern matching, not a call graph ([docs](https://ast-grep.github.io/guide/api-usage.html)).
- Mozilla's `rust-code-analysis` is MPL-2.0 and has had no release since 2023-01-13 ([crate](https://crates.io/api/v1/crates/rust-code-analysis)).

### Measured: tree-sitter, `syn` and pulldown-cmark probes

Binary sizes (stripped, LTO). The baseline is a hello-world that reads files: 319,440 B.

| Probe | Crates | Size (B) | Growth | gzip -9 |
| --- | --- | --- | --- | --- |
| Markdown links | pulldown-cmark 0.13.4 | 583,984 | +0.26 MB | n/a |
| syn | syn 3.0.9 (`full`, `visit`) + proc-macro2 `span-locations` | 1,063,920 | +0.74 MB | 492,750 |
| tree-sitter, Rust | tree-sitter 0.27.1, tree-sitter-tags 0.27.1, tree-sitter-rust 0.24.2 | 2,768,208 | +2.45 MB | 825,409 |
| tree-sitter, 16 grammar crates | above + python, javascript, typescript (ts+tsx), go, java, c, cpp, c-sharp, ruby, php, bash, json, html, css, md | 22,155,808 | +21.8 MB | 2,695,946 |
| code2graph, Rust | code2graph beta.17, `rust` feature | 2,983,456 | +2.66 MB | 927,499 |
| cgg | cgg 0.11.0 | 115,071,184 | +114.8 MB | 11,730,078 |

The compiled static grammar libraries account for almost all of the 16-grammar growth:

| Grammar | Archive size |
| --- | --- |
| json | 0.01 MB |
| html | 0.02 MB |
| css | 0.13 MB |
| go | 0.22 MB |
| core runtime | 0.29 MB |
| java | 0.41 MB |
| javascript | 0.41 MB |
| python | 0.45 MB |
| c | 0.62 MB |
| md | 0.76 MB |
| rust | 1.09 MB |
| bash | 1.33 MB |
| ruby | 2.03 MB |
| php | 2.05 MB |
| typescript+tsx | 2.77 MB |
| cpp | 3.31 MB |
| c-sharp | 5.11 MB |

Speed on tokio (556 files, 4.5 MB), one thread:

| Probe | Time | Output |
| --- | --- | --- |
| syn `parse_file` + visitor | 214–219 ms | 0 parse failures; 5,405 fn definitions; 10,967 function calls, of which 7,005 path-qualified (`a::b()`); 14,583 method calls; 6,172 macro invocations |
| tree-sitter-tags, shipped Rust query | 438–466 ms, of which 4–5 ms query setup | 6,865 definitions; 24,165 call references |
| same + one pattern for `scoped_identifier` calls | 456–461 ms | 30,914 call references (**+28%**) |
| cgg (full resolution) | 1,289 ms; 293–343 ms on the default pool | as in section 1 |
| code2graph (Rust) | ≈9.0 s | as in section 1 |

Other measurements:
- **Mixed corpus.** 804 files, 9.3 MB: CPython 3.15 `asyncio`, `email` and `json` (70 `.py` files), CodeGraph's compiled `dist` (178 `.js` files) and tokio.
  - The 16-grammar probe took 1,008–1,022 ms, of which **158–170 ms was compiling the 12 tags queries up front**.
- **Single 75 KB file** (`runtime/builder.rs`):
  - tags with Rust only: 9.0 ms;
  - syn: 2.7 ms;
  - the 16-grammar probe: 162 ms, dominated by query compilation.
- **Ambiguous names in tokio:** `fn new` is defined 194 times and `fn poll` 80 times (grep). *(inference)* A name-only caller search for a changed `new` would link every `new(` call site in the repository.

### Proposal

If Whetstone builds its own layer instead of embedding cgg *(inference throughout)*:
- **Queries.** Ship Whetstone-owned per-language queries, starting from the grammar's `tags.scm` and fixing the gaps above. Compile each query lazily, on first use of that language.
- **Calls a function makes.** Assign each call reference to the innermost definition whose byte range contains it.
- **Callers of a function.** Prefilter files by the literal name (see section 5), parse only the matches, then keep call sites whose names match. Narrow further by `Type::`/receiver qualifiers and by same-file and import proximity. Label each edge `name-only` or `qualified`.
- **First languages.** Rust and the showcase projects' languages. Each further grammar costs about 0.2–5 MB.

## 3. Rust-only parsers: `syn` and rust-analyzer

### Facts

**`syn`**
- Version 3.0.9 (2026-10-12), MIT OR Apache-2.0, MSRV 1.71; 3.0.0 shipped on 2026-07-18 ([release](https://github.com/dtolnay/syn/releases/tag/3.0.0), [Cargo.toml](https://github.com/dtolnay/syn/blob/master/Cargo.toml)).
- It is a "parsing library for parsing a stream of Rust tokens into a syntax tree" ([README](https://github.com/dtolnay/syn/blob/master/README.md)).
- Macro bodies stay as unparsed tokens ([`Macro`](https://docs.rs/syn/3.0.9/syn/struct.Macro.html)). `ExprCall` has no resolved target ([`ExprCall`](https://docs.rs/syn/3.0.9/syn/struct.ExprCall.html)).
- `parse_file` returns `Result<File>`, so one syntax error loses the whole file ([docs](https://docs.rs/syn/3.0.9/syn/fn.parse_file.html)).
- Line and column positions need proc-macro2's `span-locations` feature ([docs](https://docs.rs/proc-macro2/1.0.107/proc_macro2/struct.Span.html)).

**rust-analyzer crates (`ra_ap_*`)**
- **Cadence and licence.**
  - Releases are weekly; `ra_ap_ide` 0.0.358 came out on 2026-10-12 ([crates.io](https://crates.io/api/v1/crates/ra_ap_ide)).
  - All `ra_ap_*` crates pin each other exactly (`=0.0.358`) ([dependencies](https://crates.io/api/v1/crates/ra_ap_ide/0.0.358/dependencies)).
  - MIT OR Apache-2.0, **MSRV 1.98** ([Cargo.toml](https://github.com/rust-lang/rust-analyzer/blob/master/Cargo.toml)).
- **Stability.** "The `ide` API of rust-analyzer are explicitly unstable, but the LSP interface is stable" ([architecture](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)).
- **What loading a project needs.**
  - `load_workspace_at` runs `cargo metadata` and `rustc --print sysroot`, and optionally build scripts and the proc-macro server ([load-cargo](https://github.com/rust-lang/rust-analyzer/blob/master/crates/load-cargo/src/lib.rs), [workspace.rs](https://github.com/rust-lang/rust-analyzer/blob/master/crates/project-model/src/workspace.rs)).
  - It needs the standard library *source* (`rust-src`) ([sysroot.rs](https://github.com/rust-lang/rust-analyzer/blob/master/crates/project-model/src/sysroot.rs)).
- **Call hierarchy.**
  - `Analysis::incoming_calls` and `outgoing_calls` exist ([docs.rs](https://docs.rs/ra_ap_ide/0.0.358/ra_ap_ide/struct.Analysis.html)).
  - Incoming calls use a text search confirmed by "precise name resolution" ([search.rs](https://github.com/rust-lang/rust-analyzer/blob/master/crates/ide-db/src/search.rs)).
  - Outgoing calls use type inference ([call_hierarchy.rs](https://github.com/rust-lang/rust-analyzer/blob/master/crates/ide/src/call_hierarchy.rs)).
- **Size and cost.**
  - The rust-analyzer server binary is 36.4 MiB on macOS arm64 and 40.5 MiB on Linux x86_64 (2026-10-19 release, [releases](https://github.com/rust-lang/rust-analyzer/releases/tag/2026-10-19)).
  - Its CI `analysis-stats` takes 5.8 s and 246 MB on ripgrep, and 80 s and 1.37 GB on rust-analyzer itself ([metrics](https://github.com/rust-analyzer/metrics)).
- **Embedding cost seen elsewhere.** cargo-modules tracks rust-analyzer's MSRV exactly.
  - One ra_ap upgrade touched 41 files ([PR #389](https://github.com/regexident/cargo-modules/pull/389)).
  - A nightly cargo change broke it until the next ra_ap bump ([issue #398](https://github.com/regexident/cargo-modules/issues/398)).

### Proposal

- Do not use `syn` as the graph engine: it covers Rust only, and the requirement is any language *(inference)*.
- Do not embed `ra_ap_*`: MSRV coupling, weekly API churn, toolchain requirements and size *(inference)*.
- If precise Rust edges are wanted later, run an installed `rust-analyzer` as an optional external tool (section 4) *(inference)*.

## 4. Precise but external: SCIP, LSP and classic indexers

### Facts

**SCIP**
- The format moved to [scip-code/scip](https://github.com/scip-code/scip) (Apache-2.0, active). Sourcegraph sponsors it ([governance](https://scip-code.org/governance.html)).
- The schema has no call edges. `Occurrence.enclosing_range` is documented as useful for "call hierarchies" ([scip.proto](https://github.com/scip-code/scip/blob/main/scip.proto)).
- The `scip` reader crate is 0.10.0, Apache-2.0, and depends only on protobuf ([crates.io](https://crates.io/api/v1/crates/scip)).
- Producing an index needs an external indexer per language, each with its toolchain:
  - [scip-typescript](https://github.com/sourcegraph/scip-typescript) needs Node and `npm install`;
  - [scip-java](https://github.com/scip-code/scip-java/blob/main/docs/getting-started.md) runs a clean build;
  - [scip-python](https://github.com/sourcegraph/scip-python) needs a Python environment;
  - [scip-go](https://github.com/scip-code/scip-go) needs a Go module;
  - [scip-clang](https://github.com/sourcegraph/scip-clang) needs `compile_commands.json` and about 2 GB RAM per core;
  - `rust-analyzer scip` ([scip.rs](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/cli/scip.rs)).
- LSIF indexers from Sourcegraph are archived ([lsif-go](https://github.com/sourcegraph/lsif-go)).

**LSP**
- `callHierarchy/incomingCalls` and `callHierarchy/outgoingCalls` have existed since LSP 3.16 ([spec](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/language/callHierarchy.md)).
- Servers that implement them include:
  - rust-analyzer;
  - gopls, which excludes dynamic calls ([docs](https://github.com/golang/tools/blob/master/gopls/doc/features/navigation.md));
  - Pyright ([source](https://github.com/microsoft/pyright/blob/main/packages/pyright-internal/src/languageService/callHierarchyProvider.ts));
  - clangd ([source](https://github.com/llvm/llvm-project/blob/main/clang-tools-extra/clangd/XRefs.cpp));
  - typescript-language-server ([source](https://github.com/typescript-language-server/typescript-language-server/blob/master/src/lsp-server.ts)).
- Client-side crates:
  - `gen-lsp-types` 0.11.0 (MIT, generated from the 3.18 metamodel) is what rust-analyzer now uses ([repo](https://github.com/ribru17/gen-lsp-types));
  - `async-lsp` 0.2.4 is MIT OR Apache-2.0 ([repo](https://github.com/oxalica/async-lsp));
  - `lsp-types` 0.97.0 has not changed since 2024-06-04 ([repo](https://github.com/gluon-lang/lsp-types)).
- Cold-start figures for most servers are **UNCONFIRMED**. The gopls team measures after "initial indexing is amortized" ([Go blog](https://go.dev/blog/gopls-scalability)).

**Classic indexers**
- **Universal Ctags** is GPL-2.0. Its reference tags are "new and limited to specific areas of specific languages" ([man page](https://github.com/universal-ctags/ctags/blob/master/man/ctags.1.rst.in)).
- **GNU Global** is GPL-3.0-or-later and its references are name-based ([site](https://www.gnu.org/software/global/)).
- **Kythe** (Apache-2.0) and **Glean** (BSD-3-Clause) need build integration or a database server ([Kythe](https://github.com/kythe/kythe), [Glean](https://github.com/facebookincubator/Glean)).
- **GitHub `semantic`** is archived ([repo](https://github.com/github/semantic)).

### Proposal

*(inference throughout)*
- Keep precise edges out of the first release.
- Later, add an optional "precise mode" behind the same module boundary:
  - if a known language server is on PATH, ask it for incoming and outgoing calls;
  - otherwise read a user-supplied SCIP index with the `scip` crate.
- None of this can be the default. It needs the user's toolchain and a whole-project load, which conflicts with "any repository, locally, fast".

## 5. Files with no supported language, and non-code repositories

### Facts

**Markdown links**
- **`pulldown-cmark`** 0.13.4 (MIT) ([docs](https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/enum.Tag.html)).
  - It emits `Tag::Link { link_type, dest_url, .. }` with reference-style links already resolved.
  - Byte offsets are available through `into_offset_iter`.
  - Obsidian-style `[[wikilinks]]` are opt-in via `Options::ENABLE_WIKILINKS` ([Options](https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/struct.Options.html)).
  - *(measured)* It adds 0.26 MB. It extracted 39,516 links (3,380 repo-relative) from 1,344 Markdown files (9.4 MB) in 66–67 ms.
  - On this repo's docs it found the expected relative links, for example `docs/research/rust-stack-and-coding-standards.md -> ../adr/0001-build-agents-with-rig.md`.
  - *(measured)* With wikilinks enabled it also reported 4,569 "wikilinks" across those READMEs, which are mostly not Obsidian notes. *(inference)* Enable wikilinks only when a repo looks like a vault.
- **`comrak`** 0.56.0 (BSD-2-Clause) gives an AST with `sourcepos` and a wikilink node ([NodeValue](https://docs.rs/comrak/0.56.0/comrak/nodes/enum.NodeValue.html)).
- **`markdown`** (markdown-rs) 1.0.0 has not changed since 2025-04-23 ([repo](https://github.com/wooorm/markdown-rs)).
- **`lychee-lib`** 0.24.2 (Apache-2.0 OR MIT) extracts links from Markdown, HTML, CSS, XML and plain text ([file.rs](https://docs.rs/crate/lychee-lib/0.24.2/source/src/types/file.rs)). It also builds GitHub-style heading anchors ([markdown.rs](https://docs.rs/crate/lychee-lib/0.24.2/source/src/extract/markdown.rs)).
  - However, reqwest, hyper, tokio `full` and octocrab are non-optional dependencies ([Cargo.toml](https://docs.rs/crate/lychee-lib/0.24.2/source/Cargo.toml)).
- **Resolution rules.**
  - GitHub resolves relative links against the current file, and a leading `/` against the repository root ([GitHub Docs](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#relative-links)).
  - Obsidian resolves `[[Note]]` against the vault ([obsidian-help](https://github.com/obsidianmd/obsidian-help/blob/master/en/Linking%20notes%20and%20files/Internal%20links.md)).

**Other document formats**
- **HTML.** `href`/`src` can be read with a tokenizer such as `html5gum` 0.8.4 (MIT), which lychee uses ([crate](https://crates.io/api/v1/crates/html5gum)).
- **reStructuredText.** `.. include::` ([Docutils](https://docutils.sourceforge.io/docs/ref/rst/directives.html#include)).
- **AsciiDoc.** `include::target[]` ([Asciidoctor](https://docs.asciidoctor.org/asciidoc/latest/directives/include/)).
- **LaTeX.** `\input` and `\include` ([latexref, an unofficial manual](https://latexref.xyz/_005cinput.html)).
- I found no maintained Rust LaTeX document parser for this purpose. *(inference)* A comment-aware regex is enough for these directives.

**Config files**
- **TOML and JSON.** `toml` 1.1.8 and `serde_json` 1.0.154 are healthy ([toml](https://crates.io/api/v1/crates/toml), [serde_json](https://crates.io/api/v1/crates/serde_json)).
- **YAML.**
  - `serde_yaml` was archived and deprecated on 2024-03-25 ([release note](https://github.com/dtolnay/serde-yaml/releases/tag/0.9.34)).
  - `serde_yml` is "unsound and unmaintained" ([RUSTSEC-2025-0068](https://github.com/rustsec/advisory-db/blob/main/crates/serde_yml/RUSTSEC-2025-0068.md)).
  - `saphyr-parser` 0.2.2 (MIT OR Apache-2.0, pure Rust, YAML 1.2) emits spanned events ([docs](https://docs.rs/saphyr-parser/0.2.2/saphyr_parser/trait.SpannedEventReceiver.html)).

**Detecting what a file is**
- **Binary vs text.** Git treats a file as binary if a NUL byte appears in its first 8,000 bytes ([xdiff-interface.c](https://github.com/git/git/blob/master/xdiff-interface.c)). `.gitattributes` can override this ([gitattributes](https://git-scm.com/docs/gitattributes)).
- **Language.** GitHub Linguist's `languages.yml` (MIT) classifies 836 entries as programming, data, markup or prose ([file](https://github.com/github-linguist/linguist/blob/main/lib/linguist/languages.yml)). The Rust port `hyperpolyglot` was last released in 2020 ([crate](https://crates.io/api/v1/crates/hyperpolyglot)).

**Language-agnostic signals**
- **Literal search.**
  - ripgrep's library crates `grep-searcher`, `grep-regex` and `ignore` are Unlicense OR MIT and maintained ([grep-searcher](https://crates.io/api/v1/crates/grep-searcher), [WalkBuilder](https://docs.rs/ignore/0.4.33/ignore/struct.WalkBuilder.html)).
  - `aho-corasick` 1.1.5 matches many literals in one pass ([crate](https://crates.io/api/v1/crates/aho-corasick)).
- **Co-change (logical coupling).**
  - Gall, Hajek and Jazayeri introduced it in 1998 ([DOI](https://doi.org/10.1109/ICSM.1998.738508)).
  - Zimmermann, Weißgerber, Diehl and Zeller's ROSE "can correctly predict 26% of further files to be changed … The topmost three suggestions contain a correct location with a likelihood of 64%" ([ICSE 2004](https://thomas-zimmermann.com/publications/files/zimmermann-icse-2004.pdf)).
  - ROSE couples files "that are not even programs", for example a documentation file suggested with confidence 0.75 (same source). It ignores transactions touching more than 30 entities.
  - The TSE 2005 version reports precision above 66% at confidence 0.9 and support 3, though it then warns on only 3% of queries ([TSE 2005](https://thomas-zimmermann.com/publications/files/zimmermann-tse-2005.pdf)).
  - Prior-art tool code-maat is GPL-3.0 ([README](https://github.com/adamtornhill/code-maat#license)).
  - `gix` 0.89.0 offers `rev_walk` and `diff_tree_to_tree` with rename tracking ([Platform](https://docs.rs/gix/0.89.0/gix/revision/walk/struct.Platform.html), [Repository](https://docs.rs/gix/0.89.0/gix/struct.Repository.html), [diff Options](https://docs.rs/gix/0.89.0/gix/diff/struct.Options.html)).
- **Related tests by convention**, from official docs:

| Ecosystem | Convention | Source |
| --- | --- | --- |
| Rust | `#[cfg(test)] mod tests` in the file; integration tests in `tests/` | [Rust Book 11.3](https://doc.rust-lang.org/book/ch11-03-test-organization.html) |
| Go | `*_test.go` | [cmd/go](https://pkg.go.dev/cmd/go#hdr-Test_packages) |
| Python (pytest) | `test_*.py` or `*_test.py` | [pytest](https://docs.pytest.org/en/stable/explanation/goodpractices.html#conventions-for-python-test-discovery) |
| Jest | `__tests__/`, `*.test.*`, `*.spec.*` | [Jest](https://jestjs.io/docs/configuration#testmatch-arraystring) |
| Vitest | `*.{test,spec}.?(c\|m)[jt]s?(x)` | [Vitest](https://vitest.dev/config/include) |
| Maven/JUnit | `src/test/java`; `Test*`, `*Test`, `*Tests`, `*TestCase` | [Maven layout](https://maven.apache.org/guides/introduction/introduction-to-the-standard-directory-layout.html), [Surefire](https://maven.apache.org/surefire/maven-surefire-plugin/examples/inclusion-exclusion.html) |
| RSpec | `spec/**/*_spec.rb` | [rspec-core](https://github.com/rspec/rspec/blob/main/rspec-core/features/command_line/pattern_option.feature) |
| Elixir | `test/**/*_test.exs` | [mix test](https://hexdocs.pm/mix/Mix.Tasks.Test.html) |

### Proposal

*(inference throughout)* Non-code files should get a graph, built from five cheap providers that work in every repository.

1. **Document links.** Markdown via pulldown-cmark, plus HTML `href`/`src`, rST/AsciiDoc includes and LaTeX `\input`.
   - Resolve each target against the change's tree and keep it only if it exists.
   - Turn `#fragment` into heading-level edges using GitHub-style anchors.
   - Enable wikilinks only for vault-like repos.
2. **Path mentions.** Build an Aho-Corasick automaton over the tracked paths and distinctive basenames.
   - Changed files that mention other paths give outgoing edges.
   - Files that mention a changed path give incoming edges.
   - This also links docs, CI files and scripts to code.
3. **Related tests.** Apply the convention table first. Then fall back to test files that mention a changed symbol or path, then to co-change.
4. **Related config.** Collect the key paths and string values of changed config files and search code for them as string literals, weighted by rarity. Search the other way for config keys used in changed code. Also include the nearest build manifest (`Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`).
5. **Co-change.** Walk first-parent history in a bounded window with gix.
   - Skip merges and commits that touch more than 30 files.
   - Rank partners by confidence, with support of at least 2–3.
   - Label these edges as historical, not structural.

Files that are binary or unreadable get **no graph**, only their presence in the change. Text files with no extractor still get path-mention and co-change edges. That matches #9's "best effort" rule.

## 6. Recommendation

### Proposal

*(inference throughout)*

1. **A Whetstone-owned module boundary** (name to be settled with the glossary).
   - **Input:** a change, meaning the commit's tree plus the changed line ranges.
   - **Output:** typed edges: `calls`, `called_by`, `tests`, `config`, `links_to`, `mentions`, `co_changes_with`. Each edge carries provenance (engine and resolver) and a confidence level.
   - Engines are adapters behind it. This keeps the dependency risk below replaceable.
2. **Code edges: embed `cgg`, pinned to an exact version.**
   - Run `cgg::analyze` on the materialised tree of the change.
   - Pick the callables whose line ranges overlap changed hunks.
   - Take one hop in and one hop out from `graph.edges`.
   - Show `high` and `medium` edges and drop `low` by default.
   - Do not use `RunOptions::since`, which shells out to `git`. Compute the hunks with gix instead.
3. **Non-code edges: the five providers in section 5,** owned by Whetstone.
4. **A validation gate before release:**
   - Measure cgg's one-hop precision and recall on the showcase projects, using rust-analyzer's call hierarchy as the oracle for Rust.
   - Agree a binary-size budget.
5. **Later, optional:** a precise mode through installed language servers or SCIP (section 4).

**Main trade-off.**
- cgg buys resolved, confidence-labelled call edges in 46 languages today.
- It costs about 115 MB of binary (11.7 MB gzipped), against 2.8 MB for a Rust-only tree-sitter layer.
- It also means depending on a five-month-old, pre-1.0 crate with one main author, which fixes the whole binary on tree-sitter 0.26.

**Fallback if that is unacceptable.**
- Build the thin tree-sitter layer in section 2 for a curated language set: about 3 MB for Rust, about 22 MB for 16 grammar crates.
- Accept name-based accuracy, labelled as such. cartog's own benchmark puts heuristic-only resolution at 25–37% recall ([README](https://github.com/jrollin/cartog/blob/HEAD/README.md)).
- Revisit `code2graph` once it reaches 0.1.

**Prior proposal superseded.** This supersedes the early read in [rust-stack-and-coding-standards.md](rust-stack-and-coding-standards.md), which proposed "`tree-sitter`+`tree-sitter-rust` or `syn` 3" for a Rust-only graph. That was written before #9 widened to any repository.

## 7. Open risks and unconfirmed items

**Risks**
- **Bus factor and churn.**
  - cgg is five months old, with one author owning about 89% of commits. Its library API is pre-1.0 and `RunOptions` gains fields with new flags ([README](https://github.com/NeuralNotwerk/cgg/blob/HEAD/README.md#library)).
  - Mitigation *(inference)*: exact pin, the module boundary, and the permissive licence, which allows a fork.
- **Binary size.** cgg-lang has no grammar feature flags. Trimming needs an upstream change or a fork ([Cargo.toml](https://github.com/NeuralNotwerk/cgg/blob/HEAD/crates/cgg-lang/Cargo.toml)).
- **tree-sitter version lock.** Because of Cargo's `links` rule, every tree-sitter user in Whetstone must share cgg's 0.26 while upstream is on 0.27 *(inference from the [Cargo docs](https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key))*.
- **Materialising a commit.** cgg reads a directory, not a Git tree. Analysing a past commit needs a temporary checkout of that tree; its cost is unmeasured.
- **Walker deny list.** It matches any ancestor path component (`.cargo`, `dist`, `node_modules`, …). A repository under such a path, or code in a `dist/` folder, yields no graph *(measured)*.
- **Accuracy not yet validated.** Evidence so far is one hand-checked one-hop sample. No oracle comparison has been run.
- **Scale.** cgg has no cache and re-parses the whole tree on every run. Upstream's largest timing is about 21k callables in 0.39 s. Monorepo-scale behaviour is **UNCONFIRMED**.
- **Name-based fallbacks over-connect** (194 definitions of `fn new` in tokio). Edges must carry their confidence level into the evidence so the generator and Jev can discount them *(inference)*.
- **Licence notices.**
  - Apache-2.0-only dependencies (code2graph, `scip`) and BSD-2 (comrak) need their notices shipped.
  - cgg is dual-licensed and states its whole tree is permissive.
  - Not yet checked with `cargo-deny`.
- **Measurement scope.** All sizes and timings come from one Apple M4 Max with rustc 1.93.1. Linux and Windows sizes are not measured.

**UNCONFIRMED**
- The size the tree-sitter `wasm` feature adds, and how much slower WASM grammars parse.
- Per-grammar licences inside `tree-sitter-language-pack`.
- Whether any stack-graphs fork is maintained.
- Cold-start times of Pyright, tsserver and clangd for one call-hierarchy query.
- The abstract of Gall et al. 1998 (only the DOI and bibliographic data were verified).
