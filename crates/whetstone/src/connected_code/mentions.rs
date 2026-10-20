//! Path mentions: the repository files whose paths the change names on the
//! lines it adds or modifies.

use std::collections::BTreeSet;

use aho_corasick::AhoCorasick;

use super::touched_text::{self, Target, TouchedText};

/// The commit's paths are more than one search can hold.
#[derive(Debug, thiserror::Error)]
#[error("the commit's paths are too many to search for mentions")]
pub(crate) struct TooManyPaths(#[from] aho_corasick::BuildError);

/// The repository files whose paths the touched text files name on the
/// lines the change adds or modifies, one per path, in path order.
///
/// `files` are the paths of the files in the commit's tree, and `touched`
/// every file the change touches. Each target is sent whole, in the medium
/// tier, and each touched file it lists is one it is "named in".
///
/// One search looks for every path in the tree that holds a `/`, so a bare
/// file name such as `limits.toml`, or a name at the root such as
/// `README`, never counts. A path counts on a changed line of a touched
/// file that is text at the commit, where it stands alone, not inside a
/// longer path or word: the characters right before and after it are not
/// letters, digits or `_ - . / \ ~`. A `./` or a `/` right before it, with
/// no such character before them, and dots ending a sentence right after it
/// are allowed. A touched file is never a target.
///
/// Fails only when the tree has more paths than one search can hold.
pub(crate) fn named_files(
    files: &BTreeSet<String>,
    touched: &[TouchedText<'_>],
) -> Result<Vec<Target>, TooManyPaths> {
    let paths: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|path| path.contains('/'))
        .collect();
    let search = AhoCorasick::new(&paths)?;
    let touched_paths: BTreeSet<&str> = touched.iter().map(TouchedText::path).collect();
    let mut found = Vec::new();
    for file in touched {
        for line in file.changed_lines() {
            for named in search.find_overlapping_iter(line) {
                let path = paths[named.pattern().as_usize()];
                if stands_alone(line, named.start(), named.end()) && !touched_paths.contains(path) {
                    found.push((path.to_owned(), file.path()));
                }
            }
        }
    }
    Ok(touched_text::targets(found))
}

/// Whether the bytes `start..end` of `line` stand alone, not inside a
/// longer path or word: no character that carries a path or word on comes
/// right before or after them. A `./` or a `/` right before them names the
/// same path from the root, as a command or a root-relative path does.
/// Dots right after them end a sentence, unless such a character follows
/// the dots.
fn stands_alone(line: &str, start: usize, end: usize) -> bool {
    let before = line.get(..start).unwrap_or_default();
    let before = before
        .strip_suffix("./")
        .or_else(|| before.strip_suffix('/'))
        .unwrap_or(before);
    let after = line.get(end..).unwrap_or_default();
    let starts_alone = before
        .chars()
        .next_back()
        .is_none_or(|c| !continues_path(c));
    let ends_alone = after
        .trim_start_matches('.')
        .chars()
        .next()
        .is_none_or(|c| !continues_path(c));
    starts_alone && ends_alone
}

/// Whether `c` carries a path or word on: a letter, a digit, or one of
/// `_ - . / \ ~`.
fn continues_path(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\' | '~')
}

#[cfg(test)]
mod tests {
    use whetstone_core::{FileContent, TouchedFile};

    use super::*;

    fn tree(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|&path| path.to_owned()).collect()
    }

    fn created(path: &str, text: &str) -> TouchedFile {
        TouchedFile::created(path.to_owned(), text.to_owned())
    }

    /// The touched `file`, every line of which the change adds.
    fn all_added(file: &TouchedFile) -> TouchedText<'_> {
        let lines = file
            .after()
            .and_then(|after| after.text())
            .map_or(0, |text| text.lines().count());
        TouchedText::new(file, (1..=lines).collect())
    }

    fn target(path: &str, found_in: &[&str]) -> Target {
        Target {
            path: path.to_owned(),
            found_in: found_in.iter().map(|&path| path.to_owned()).collect(),
        }
    }

    fn named(files: &BTreeSet<String>, touched: &[TouchedText<'_>]) -> Vec<Target> {
        named_files(files, touched).expect("a few paths are not too many")
    }

    #[test]
    fn a_path_named_on_a_changed_line_is_found() {
        let deploy = created(
            ".github/workflows/deploy.yml",
            "steps:\n  - run: scripts/release.sh --limits config/limits.toml\n",
        );
        let files = tree(&[
            ".github/workflows/deploy.yml",
            "config/limits.toml",
            "scripts/release.sh",
            "src/main.rs",
        ]);

        let found = named(&files, &[all_added(&deploy)]);

        assert_eq!(
            found,
            [
                target("config/limits.toml", &[".github/workflows/deploy.yml"]),
                target("scripts/release.sh", &[".github/workflows/deploy.yml"]),
            ]
        );
    }

    #[test]
    fn a_bare_file_name_is_not_a_mention() {
        let notes = created(
            "docs/notes.md",
            "Raise the cap in limits.toml, then update the README and Makefile.\n",
        );
        let files = tree(&["Makefile", "README", "config/limits.toml", "docs/notes.md"]);

        let found = named(&files, &[all_added(&notes)]);

        assert_eq!(found, []);
    }

    #[test]
    fn a_path_inside_a_longer_path_or_word_is_not_a_mention() {
        let notes = created(
            "docs/notes.md",
            "Not app/config/limits.toml, myconfig/limits.toml or ../config/limits.toml,\n\
             nor config/limits.toml.bak, config/limits.tomlx, config/limits.toml/old,\n\
             config/limits.toml-v2, ~config/limits.toml or .config/limits.toml.\n",
        );
        let files = tree(&["config/limits.toml", "docs/notes.md"]);

        let found = named(&files, &[all_added(&notes)]);

        assert_eq!(found, []);
    }

    #[test]
    fn a_path_written_from_the_root_with_a_slash_is_a_mention() {
        let notes = created("notes.md", "Raise the cap in /config/limits.toml first.\n");
        let links = created(
            "links.md",
            "Not /etc/config/limits.toml, //config/limits.toml or\n\
             https://example.com/config/limits.toml.\n",
        );
        let files = tree(&["config/limits.toml", "links.md", "notes.md"]);

        let found = named(&files, &[all_added(&notes), all_added(&links)]);

        assert_eq!(found, [target("config/limits.toml", &["notes.md"])]);
    }

    #[test]
    fn punctuation_around_a_path_does_not_hide_it() {
        let notes = created(
            "notes.md",
            "Edit `config/limits.toml` (and scripts/release.sh), as \"docs/guide.md#limits\"\n\
             says; src/main.rs:42 reads it, ./scripts/check.sh checks it,\n\
             --rules=lint/rules.toml sets the rules and “docs/faq.md” explains tools/lint.py.\n",
        );
        let files = tree(&[
            "config/limits.toml",
            "docs/faq.md",
            "docs/guide.md",
            "lint/rules.toml",
            "notes.md",
            "scripts/check.sh",
            "scripts/release.sh",
            "src/main.rs",
            "tools/lint.py",
        ]);

        let found = named(&files, &[all_added(&notes)]);

        assert_eq!(
            found,
            [
                target("config/limits.toml", &["notes.md"]),
                target("docs/faq.md", &["notes.md"]),
                target("docs/guide.md", &["notes.md"]),
                target("lint/rules.toml", &["notes.md"]),
                target("scripts/check.sh", &["notes.md"]),
                target("scripts/release.sh", &["notes.md"]),
                target("src/main.rs", &["notes.md"]),
                target("tools/lint.py", &["notes.md"]),
            ]
        );
    }

    #[test]
    fn a_mention_on_a_line_the_change_does_not_add_or_modify_is_ignored() {
        let deploy = TouchedFile::modified(
            "ci/deploy.yml".to_owned(),
            "run: scripts/release.sh\nlimits: none\n".to_owned(),
            "run: scripts/release.sh\nlimits: config/limits.toml\n".to_owned(),
        );
        let files = tree(&["ci/deploy.yml", "config/limits.toml", "scripts/release.sh"]);

        let found = named(&files, &[TouchedText::new(&deploy, BTreeSet::from([2]))]);

        assert_eq!(found, [target("config/limits.toml", &["ci/deploy.yml"])]);
    }

    #[test]
    fn a_touched_file_is_never_named_and_a_target_lists_every_file_naming_it() {
        let deploy = created(
            "ci/deploy.yml",
            "run: scripts/release.sh --notes docs/release.md --logo assets/logo.png\n",
        );
        let release = created(
            "docs/release.md",
            "Run scripts/release.sh, which loads scripts/common.sh.\n",
        );
        let script = TouchedFile::modified(
            "scripts/release.sh".to_owned(),
            "#!/bin/sh\n".to_owned(),
            "#!/bin/sh\n. scripts/common.sh\n".to_owned(),
        );
        let logo = TouchedFile::created(
            "assets/logo.png".to_owned(),
            FileContent::Binary { size: 2048 },
        );
        let files = tree(&[
            "assets/logo.png",
            "ci/deploy.yml",
            "docs/release.md",
            "scripts/common.sh",
            "scripts/release.sh",
        ]);

        let found = named(
            &files,
            &[
                all_added(&script),
                all_added(&deploy),
                all_added(&logo),
                all_added(&release),
            ],
        );

        assert_eq!(
            found,
            [target(
                "scripts/common.sh",
                &["docs/release.md", "scripts/release.sh"]
            )]
        );
    }
}
