//! Rig crates move together (`CODING_STANDARDS.md` rule 10). The AI crate
//! pins `rig` exactly, but `rig` asks for its own crates with `^0.45.0`, so a
//! `cargo update` of one of them could move it alone. Only the lockfile
//! holds them together, and this test keeps it so.

use serde::Deserialize;

#[derive(Deserialize)]
struct Lockfile {
    package: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
}

#[test]
fn every_rig_crate_in_the_lockfile_is_at_rigs_version() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let lockfile: Lockfile =
        toml::from_str(&std::fs::read_to_string(path).expect("the workspace has a lockfile"))
            .expect("the lockfile is TOML");
    let rig: Vec<&Package> = lockfile
        .package
        .iter()
        .filter(|package| package.name == "rig" || package.name.starts_with("rig-"))
        .collect();
    let [version] = rig
        .iter()
        .filter(|package| package.name == "rig")
        .map(|package| package.version.as_str())
        .collect::<Vec<_>>()[..]
    else {
        panic!("the lockfile holds one version of rig");
    };

    let apart: Vec<String> = rig
        .iter()
        .filter(|package| package.version != version)
        .map(|package| format!("{} {}", package.name, package.version))
        .collect();
    assert!(
        apart.is_empty(),
        "these Rig crates are not at rig {version}: {apart:?}"
    );
    assert!(
        rig.iter().any(|package| package.name == "rig-core"),
        "the lockfile holds rig-core"
    );
}
