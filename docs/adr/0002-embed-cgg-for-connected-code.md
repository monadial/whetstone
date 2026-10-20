---
status: accepted
---

# Embed cgg for connected code

Whetstone finds the code a change calls and the code that calls it with the `cgg` crate, embedded as an ordinary dependency and pinned to an exact version. Whetstone accepts any Git repository, and `cgg` was the only library that embeds in the CLI and resolves calls by scope, imports and receiver type across many languages (46), with a confidence level on every edge. A layer of our own on tree-sitter would have been 10–30 MB instead of about 115 MB, but it would match calls by name only, so a changed `new` would connect to every `new(` in the repository and send unrelated code. We accepted the size and the risk of a young dependency with one main maintainer, and put `cgg` behind a module that Whetstone owns so the engine can be swapped.

## Considered options

- Our own tree-sitter layer for a short list of languages: smaller and fully under our control, but name-matched, and the grammars' shipped tags queries miss calls in several languages.
- `code2graph`: smaller, but pre-0.1, and most of its edges on tokio were name-only or heuristic.
- `syn` and rust-analyzer's crates: Rust only; rust-analyzer's API also changes weekly and needs a full toolchain.
- CodeGraph and the other tools compared on tokenade.net: none ships as an embeddable, permissively licensed crate.

The measurements are in the research note [Dependency graph tooling for a change](../research/dependency-graph-tooling.md).

## Consequences

- The release binary grows from about 7 MB to about 122 MB (about 12 MB more to download). None of `cgg`'s grammars is optional, so shrinking it needs an upstream change or a fork.
- Cargo allows one tree-sitter per binary, so any other tree-sitter use in Whetstone, such as syntax highlighting, must use the tree-sitter version `cgg` pins (0.26 in `cgg` 0.11.0).
- `cgg` reads directories, not Git objects, so a commit's tree is written to a temporary directory before it is analysed.
- `cgg` is pre-1.0 and its options gain fields with new releases. Upgrade it on purpose, and check the showcase project's expected connected code on every upgrade.
