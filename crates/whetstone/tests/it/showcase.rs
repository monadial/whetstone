//! The embedded showcase project: the build step turns its patch series into
//! data in the binary, so Practice needs no git and no files at runtime.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use whetstone_core::{Area, AreaId, FileContent, TouchedFile};

#[test]
fn the_embedded_showcase_has_all_ten_curated_changes_with_the_area_ids_their_trailers_name() {
    let showcase = whetstone::embedded_showcase();

    let changes: Vec<(&str, Vec<&str>)> = showcase
        .changes()
        .iter()
        .map(|change| {
            (
                change.summary(),
                change.areas().iter().map(AreaId::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("Model the payment lifecycle", vec!["domain"]),
            (
                "Persist payments in an append-only file store",
                vec!["infrastructure"]
            ),
            (
                "Call the payment provider with timeouts and retries",
                vec!["infrastructure"]
            ),
            ("Serve payments over HTTP", vec!["domain", "infrastructure"]),
            ("Preserve payment retry identity", vec!["domain"]),
            (
                "Put storage and the provider behind ports",
                vec!["system-design"]
            ),
            (
                "Trace requests and export payment metrics",
                vec!["observability"]
            ),
            (
                "Cache payments per tenant",
                vec!["scalability", "infrastructure"]
            ),
            ("Rename ChargeOutcome to ProviderOutcome", vec!["domain"]),
            ("Run rustfmt over the HTTP layer", vec!["infrastructure"]),
        ]
    );
}

/// The showcase project's code, as the repository holds it.
fn project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../showcase/payments")
}

#[test]
fn the_showcase_project_ships_no_area_files_and_offers_the_defaults() {
    let config: toml::Table = fs::read_to_string(project().join(".whetstone/config.toml"))
        .expect("read the showcase project's config")
        .parse()
        .expect("the config is TOML");
    assert!(config.is_empty(), "the config sets nothing: {config:#?}");
    assert!(
        !project().join(".whetstone/areas").exists(),
        "the showcase project has no area files"
    );

    let showcase = whetstone::embedded_showcase();

    assert_eq!(showcase.areas(), Area::defaults());
}

#[test]
fn replaying_every_embedded_change_rebuilds_the_showcase_projects_files_exactly() {
    let mut replayed = BTreeMap::new();
    for change in whetstone::embedded_showcase().changes() {
        for file in change.files() {
            assert_eq!(
                replayed.get(file.path()).map(String::as_str),
                file.before().and_then(FileContent::text),
                "{} before {:?}",
                file.path(),
                change.summary()
            );
            match file.after().and_then(FileContent::text) {
                Some(after) => replayed.insert(file.path().to_owned(), after.to_owned()),
                None => replayed.remove(file.path()),
            };
        }
    }

    // `showcase/check-history.sh` checks that the tree has no other files.
    assert!(replayed.contains_key("src/payment.rs"), "{replayed:#?}");
    for (path, contents) in &replayed {
        let on_disk = fs::read_to_string(project().join(path))
            .unwrap_or_else(|error| panic!("read {path} in the showcase project: {error}"));
        assert!(*contents == on_disk, "{path} differs from the project");
    }
}

#[test]
fn an_embedded_change_keeps_its_whole_message_without_the_areas_trailer_and_its_diff() {
    let showcase = whetstone::embedded_showcase();
    let rustfmt = showcase.changes().last().expect("ten changes");

    assert_eq!(
        rustfmt.message(),
        "Run rustfmt over the HTTP layer\n\n\
         The metrics handler and the error response went in without cargo fmt.\n\
         Format them. No code changes."
    );
    let patch = fs::read_to_string(
        project().join("../payments-history/0010-Run-rustfmt-over-the-HTTP-layer.patch"),
    )
    .expect("read the change's patch");
    let (_, diff) = patch
        .split_once("\n\ndiff --git ")
        .expect("the patch has a diff after its diffstat");
    assert_eq!(rustfmt.diff(), format!("diff --git {diff}"));
    assert_eq!(
        rustfmt
            .files()
            .iter()
            .map(TouchedFile::path)
            .collect::<Vec<_>>(),
        ["src/http.rs"]
    );
}

#[test]
fn each_file_an_embedded_change_touches_has_its_own_diff_in_the_same_order() {
    for change in whetstone::embedded_showcase().changes() {
        let headers: Vec<&str> = change
            .file_diffs()
            .map(|diff| diff.lines().next().unwrap_or_default())
            .collect();
        let expected: Vec<String> = change
            .files()
            .iter()
            .map(|file| format!("diff --git a/{0} b/{0}", file.path()))
            .collect();
        assert_eq!(headers, expected, "{:?}", change.summary());
    }
}
