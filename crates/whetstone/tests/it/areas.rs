//! Whetstone's default areas: the area files in its source, embedded in the
//! binary, which the build checks.

use std::error::Error as _;
use std::fs;
use std::path::Path;

use whetstone_core::{Area, AreaId};

use crate::build_areas;

#[test]
fn the_embedded_default_area_files_are_exactly_the_files_in_the_source() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../whetstone-core/areas");
    let mut on_disk: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
        .expect("read the default area files")
        .map(|entry| {
            let path = entry.expect("a directory entry").path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a UTF-8 file name")
                .to_owned();
            (name, fs::read(&path).expect("read a default area file"))
        })
        .collect();
    on_disk.sort();

    let embedded: Vec<(String, Vec<u8>)> = Area::default_files()
        .iter()
        .map(|&(name, contents)| (name.to_owned(), contents.to_vec()))
        .collect();

    assert_eq!(
        embedded.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        on_disk.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "`showcase/check-history.sh` reads the IDs from the directory"
    );
    assert!(embedded == on_disk, "the embedded contents match the files");
}

#[test]
fn the_build_takes_the_ids_of_the_default_area_files_which_are_all_valid() {
    let ids = build_areas::default_ids(Area::default_files()).expect("the defaults are valid");

    assert_eq!(
        ids.iter().map(AreaId::as_str).collect::<Vec<_>>(),
        [
            "domain",
            "infrastructure",
            "observability",
            "scalability",
            "system-design"
        ]
    );
}

#[test]
fn the_build_fails_on_a_default_area_file_that_is_invalid() {
    let domain = Area::default_files()[0];
    let without_a_description: &[u8] = b"+++\nname = \"Infrastructure\"\n+++\n\nLook at it.\n";

    let error = build_areas::default_ids(&[domain, ("infrastructure.md", without_a_description)])
        .expect_err("a default area file is invalid");

    assert_eq!(
        error.to_string(),
        "infrastructure.md is not a valid area file"
    );
    assert_eq!(
        error.source().map(ToString::to_string).as_deref(),
        Some("the front matter has no `description`")
    );
}
