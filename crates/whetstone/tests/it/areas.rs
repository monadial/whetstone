//! Whetstone's default areas: the area files in its source, embedded in the
//! binary.

use std::fs;
use std::path::Path;

use whetstone_core::Area;

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
