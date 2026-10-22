---
status: accepted
---

# Version by SemVer, and release with dist

## Versions

Whetstone's version is the workspace version in the root `Cargo.toml`, and nothing else sets it. The version follows Semantic Versioning, and a release is the tag `v<version>` on a commit on `main`. Before 1.0:

- a fix bumps the patch;
- a feature, or a breaking change, bumps the minor.

A change is breaking when it changes something a learner keeps or scripts:

- keys in `config.toml` or in `.whetstone/config.toml`;
- the area file format;
- `labels.toml`'s format;
- flags, subcommands or exit codes;
- where Whetstone keeps its files.

A prerelease is `-rc.N`, such as `0.1.0-rc.1`.

`whetstone --version` prints the version, then the short hash and date of the commit the binary was built from: `whetstone 0.1.0 (04d5bb5 2026-10-21)`. `-V` prints the version alone. The date is the commit's, not the build's, so building one commit twice gives the same output. A build without git says the version alone.

## Channels

A release publishes to GitHub Releases:

- archives for Linux and macOS, on x86_64 and arm64;
- their SHA-256 checksums;
- build attestations;
- a shell installer that verifies the checksum.

The README also documents `cargo install --locked --git` for everything else.

[dist](https://github.com/axodotdev/cargo-dist), pinned to an exact version, generates the release workflow from its config. Every build job waits on `.github/workflows/live.yml`, which dist runs as a plan job: a release can't go out unless Jev and the generator answer live. Each target builds on its own native runner, as CI does, because the connected-code grammars and the TLS library compile C.

## Considered options

- **`0.1.<CI build number>`.** Every build on `main` would become a release, and each CI run would burn a number, so numbers would skip and say nothing about what changed. A patch bump would no longer mean "fixes only". `Cargo.toml` couldn't hold the version, so a local build would disagree with CI. The commit in `--version` traces a binary to its source without a build number.
- **Nightly prereleases from `main`.** More builds to try early, but more Actions minutes and more releases to scroll past. Nobody needs them yet.
- **crates.io.** The name `whetstone` is taken there by an unrelated crate. `build.rs` also reads the showcase project from outside the crate, which `cargo publish` doesn't package. Every crate stays unpublishable (rule 56).
- **Homebrew.** A tap needs a repository of its own and a token that can push to it. homebrew-core needs notability, and it builds from source. Not now.
- **A hand-written release workflow,** or GoReleaser. A hand-written workflow could carry this repository's comments, but its installer and archive layout would be ours to maintain. GoReleaser's Rust support cross-compiles with Zig, which is less certain with C dependencies than native runners.

## Consequences

- A release PR bumps the version in `Cargo.toml`, together with `Cargo.lock`, and dates the version's `CHANGELOG.md` section, which becomes the release notes. dist refuses a tag that doesn't match the version.
- Builds between releases carry the last release's version, and the commit tells them apart.
- dist owns `.github/workflows/release.yml` and regenerates it from its config. The reasons for each setting live in the config file's comments, and a check fails when the generated workflow is out of date.
- The release gate needs the `OPENROUTER_API_KEY` secret ([#96](https://github.com/monadial/whetstone/issues/96)). Without it, the release fails rather than going out unchecked.
