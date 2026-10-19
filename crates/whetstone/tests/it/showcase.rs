//! The embedded showcase project: the build step turns its patch series into
//! data in the binary, so Practice needs no git and no files at runtime.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use whetstone_core::Area;

#[test]
fn the_embedded_showcase_has_all_ten_curated_changes_with_the_areas_their_trailers_declare() {
    let showcase = whetstone::embedded_showcase();

    let changes: Vec<(&str, Vec<&str>)> = showcase
        .changes()
        .iter()
        .map(|change| {
            (
                change.summary(),
                change.areas().iter().map(Area::name).collect(),
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
                vec!["system design"]
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
fn the_embedded_showcase_offers_the_areas_its_config_declares() {
    let config: toml::Table = fs::read_to_string(project().join(".whetstone/config.toml"))
        .expect("read the showcase project's config")
        .parse()
        .expect("the config is TOML");
    let declared: Vec<&str> = config["areas"]
        .as_array()
        .expect("the config declares areas")
        .iter()
        .map(|area| area.as_str().expect("an area name"))
        .collect();

    let showcase = whetstone::embedded_showcase();

    assert_eq!(
        showcase.areas().iter().map(Area::name).collect::<Vec<_>>(),
        declared
    );
}

#[test]
fn replaying_every_embedded_change_rebuilds_the_showcase_project_exactly() {
    let mut replayed = BTreeMap::new();
    for change in whetstone::embedded_showcase().changes() {
        for file in change.files() {
            assert_eq!(
                replayed.get(file.path()).map(String::as_str),
                file.before(),
                "{} before {:?}",
                file.path(),
                change.summary()
            );
            match file.after() {
                Some(after) => replayed.insert(file.path().to_owned(), after.to_owned()),
                None => replayed.remove(file.path()),
            };
        }
    }

    let project = project();
    let mut on_disk = BTreeMap::new();
    read_tree(&project, &project, &mut on_disk);
    assert_eq!(
        replayed.keys().collect::<Vec<_>>(),
        on_disk.keys().collect::<Vec<_>>()
    );
    for (path, contents) in &on_disk {
        assert!(
            replayed[path] == *contents,
            "{path} differs from the project"
        );
    }
}

/// Reads every file under `dir` into `files`, keyed by its path relative to
/// `root`, leaving out what the project's `.gitignore` ignores.
fn read_tree(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(dir).expect("read the showcase project") {
        let path = entry.expect("read the showcase project").path();
        let relative = path
            .strip_prefix(root)
            .expect("under the project")
            .to_str()
            .expect("a UTF-8 path")
            .to_owned();
        if ["target", "data"].contains(&relative.as_str()) {
            continue;
        }
        if path.is_dir() {
            read_tree(root, &path, files);
        } else {
            files.insert(relative, fs::read_to_string(&path).expect("a text file"));
        }
    }
}
