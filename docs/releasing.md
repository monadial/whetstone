# Releasing Whetstone

This is how the maintainer cuts a release. [ADR 0003](adr/0003-version-by-semver-and-release-with-dist.md) says why releases work this way, and `dist-workspace.toml` configures the Release workflow.

The Release workflow runs the live tests first, as its gate, so the `OPENROUTER_API_KEY` repository secret must be set ([#96](https://github.com/monadial/whetstone/issues/96)). Without it, the release fails before anything is built.

## 1. Choose the version

Read `CHANGELOG.md`'s `Unreleased` section. Before 1.0:

- a release of fixes alone bumps the patch: 0.1.0 to 0.1.1;
- a release with a feature, or a breaking change, bumps the minor: 0.1.1 to 0.2.0. ADR 0003 lists what counts as breaking.

A prerelease, such as one to try a risky change, adds `-rc.N`: `0.2.0-rc.1`.

## 2. Open a release PR

On a branch off `main`:

1. Set `version` under `[workspace.package]` in `Cargo.toml`, then run `cargo check --workspace`, so `Cargo.lock` follows.
2. For a release, rename `## Unreleased` in `CHANGELOG.md` to `## <version> - <YYYY-MM-DD>`, and start a new, empty `## Unreleased` above it. For a prerelease, leave `Unreleased` as it is. dist takes the release notes from that section.
3. Build the branch, and go through the [pre-release terminal checklist](terminal-checklist.md) in each supported terminal. A prerelease can skip this step.
4. Merge the PR once CI passes.

## 3. Tag it

Tag the commit the PR landed as on `main`. Only admins can push `v*` tags (the `release tags` ruleset).

```sh
git switch main
git pull --ff-only
git tag -a v<version> -m "Whetstone <version>"
git push origin v<version>
```

## 4. Watch the Release workflow

The tag starts the Release workflow:

1. **plan** checks that the tag matches the version in `Cargo.toml`.
2. **custom-live** runs the live tests, as the release gate.
3. **build-local-artifacts** builds each platform on its own runner. It writes the third-party licenses first, and attests each archive after.
4. **build-global-artifacts** writes the installer and the checksums.
5. **host** publishes the GitHub Release, with every file attached at once. A `-rc.N` tag publishes a prerelease, which `releases/latest` skips.

Releases are immutable. Once a release is published, its files and its tag can't change, and its version can't be published again.

If a job fails before **host**, nothing is published:

1. delete the tag, with `git push origin :refs/tags/v<version>` and `git tag -d v<version>`;
2. fix the cause in a PR;
3. tag again.

A problem found after **host** is fixed in the next patch release.

## 5. Check the release

On Linux and on macOS:

1. Install with the installer command on the release's page.
2. Check that `whetstone --version` names the version and the tagged commit.
3. Download one archive, and check it with `gh attestation verify <archive> --repo monadial/whetstone`.

## Upgrade dist

dist generates `.github/workflows/release.yml`, so never edit that file by hand. To move to a new dist version:

1. Install it, and set `cargo-dist-version` in `dist-workspace.toml`.
2. Run `dist generate`.
3. Pin any action the new workflow uses to a full commit SHA under `[dist.github-action-commits]`, then run `dist generate` again.
4. Update the dist version in `ci.yml`'s "Release config" job.
