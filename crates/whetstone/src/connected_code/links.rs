//! Markdown links: the repository files that touched Markdown links to on
//! the lines the change adds or modifies.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired into the outbound review by the #62 integration"
    )
)]

use std::collections::BTreeSet;
use std::path::Path;

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag};

use super::touched_text::{self, Target, TouchedText};

/// The repository files that the touched Markdown files link to on the
/// lines the change adds or modifies, one per path, in path order.
///
/// `files` are the paths of the files in the commit's tree, and `touched`
/// every file the change touches. Each target is sent whole, in the high
/// tier, and each touched file it lists is one it is "linked from".
///
/// Markdown files are those ending `.md` or `.markdown`. Their links are
/// read as the Markdown specification defines them, with reference links
/// resolved through their definitions; images and wikilinks are not links.
/// A link counts when any line it spans is one the change adds or modifies.
/// Its destination resolves against the linking file, or against the
/// repository's root when it starts with `/`, without its query or
/// fragment. Only a file in the commit's tree that the change does not
/// touch is a target: an external URL, a missing file, a directory and a
/// path outside the repository are not.
pub(crate) fn linked_files(files: &BTreeSet<String>, touched: &[TouchedText<'_>]) -> Vec<Target> {
    let touched_paths: BTreeSet<&str> = touched.iter().map(TouchedText::path).collect();
    let mut found = Vec::new();
    for file in touched.iter().filter(|file| is_markdown(file.path())) {
        for destination in changed_destinations(file) {
            let Some(path) = resolve(file.path(), &destination) else {
                continue;
            };
            if files.contains(&path) && !touched_paths.contains(path.as_str()) {
                found.push((path, file.path()));
            }
        }
    }
    touched_text::targets(found)
}

/// Whether the file at `path` is Markdown: its extension is `.md` or
/// `.markdown`, in any case.
fn is_markdown(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}

/// The destinations of the links in `file` that lie on a line the change
/// adds or modifies.
fn changed_destinations<'a>(file: &TouchedText<'a>) -> Vec<CowStr<'a>> {
    let Some(text) = file.text() else {
        return Vec::new();
    };
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    // The 1-based line that the byte at `offset` lies on.
    let line_of = |offset: usize| line_starts.partition_point(|&start| start <= offset);
    Parser::new_ext(text, Options::empty())
        .into_offset_iter()
        .filter_map(|(event, span)| match event {
            // An email autolink's destination is an address with no
            // `mailto:` scheme.
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) if link_type != LinkType::Email => {
                let lines = line_of(span.start)..=line_of(span.end.saturating_sub(1));
                lines
                    .into_iter()
                    .any(|line| file.is_changed(line))
                    .then_some(dest_url)
            }
            _ => None,
        })
        .collect()
}

/// The repository path that a link in the file at `linking` to
/// `destination` points to, if it stays inside the repository. Its query
/// and fragment are dropped, and its percent-encoding decoded. A
/// destination left empty points to the linking file itself, and one ending
/// `/` to a directory. A URL with a scheme, or one starting `//`, which
/// names a host, points outside.
fn resolve(linking: &str, destination: &str) -> Option<String> {
    if destination.starts_with("//") || has_scheme(destination) {
        return None;
    }
    let path = destination
        .split(['?', '#'])
        .next()
        .filter(|path| !path.is_empty() && !path.ends_with('/'))?;
    let path = percent_decoded(path)?;
    let (directory, path) = match path.strip_prefix('/') {
        Some(from_root) => ("", from_root),
        None => (
            linking
                .rsplit_once('/')
                .map_or("", |(directory, _)| directory),
            path.as_str(),
        ),
    };
    let mut parts = Vec::new();
    for part in directory.split('/').chain(path.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Whether `destination` starts with a URL scheme, such as `https:`: a
/// letter, then letters, digits, `+`, `-` or `.`, then a colon.
fn has_scheme(destination: &str) -> bool {
    destination.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// `path` with each `%` and two hexadecimal digits replaced by the byte they
/// encode, if the result is UTF-8.
fn percent_decoded(path: &str) -> Option<String> {
    let hex = |digit: u8| {
        char::from(digit)
            .to_digit(16)
            .and_then(|value| u8::try_from(value).ok())
    };
    let mut bytes = path.bytes();
    let mut decoded = Vec::with_capacity(path.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let mut ahead = bytes.clone();
            if let (Some(high), Some(low)) =
                (ahead.next().and_then(hex), ahead.next().and_then(hex))
            {
                decoded.push(high * 16 + low);
                bytes = ahead;
                continue;
            }
        }
        decoded.push(byte);
    }
    String::from_utf8(decoded).ok()
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

    #[test]
    fn a_relative_link_resolves_against_the_linking_file() {
        let guide = created(
            "docs/guide.md",
            "Read [setup](setup.md) and [the rules](../CONTRIBUTING.md).\n\
             Then [the next step](./steps/next.md).\n",
        );
        let files = tree(&[
            "CONTRIBUTING.md",
            "docs/guide.md",
            "docs/setup.md",
            "docs/steps/next.md",
        ]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(
            found,
            [
                target("CONTRIBUTING.md", &["docs/guide.md"]),
                target("docs/setup.md", &["docs/guide.md"]),
                target("docs/steps/next.md", &["docs/guide.md"]),
            ]
        );
    }

    #[test]
    fn a_link_starting_with_a_slash_resolves_against_the_repository_root() {
        let guide = created(
            "docs/guide.md",
            "The limits live in [limits](/config/limits.toml).\n",
        );
        let files = tree(&["config/limits.toml", "docs/config/limits.toml"]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(found, [target("config/limits.toml", &["docs/guide.md"])]);
    }

    #[test]
    fn a_link_s_fragment_and_query_are_dropped() {
        let guide = created(
            "docs/guide.md",
            "See [install](setup.md#install), [raw](faq.md?plain=1#top)\n\
             and [above](#intro).\n",
        );
        let files = tree(&["docs/faq.md", "docs/guide.md", "docs/setup.md"]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(
            found,
            [
                target("docs/faq.md", &["docs/guide.md"]),
                target("docs/setup.md", &["docs/guide.md"]),
            ]
        );
    }

    #[test]
    fn an_external_link_is_ignored_whatever_its_scheme() {
        let guide = created(
            "docs/guide.md",
            "See [the site](https://example.com/docs/setup.md),\n\
             [the mirror](//example.com/setup.md), <https://example.com>,\n\
             [notes](notes:setup.md) and <team@example.com>.\n",
        );
        // Even where a file at the address exists.
        let files = tree(&[
            "docs/guide.md",
            "docs/notes:setup.md",
            "docs/setup.md",
            "docs/team@example.com",
            "example.com/setup.md",
        ]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(found, []);
    }

    #[test]
    fn a_missing_target_a_directory_and_a_path_outside_the_repository_are_ignored() {
        let guide = created(
            "docs/guide.md",
            "See [the plan](plan.md), [the code](../src), [the code](../src/),\n\
             [setup](setup.md/) and [elsewhere](../../README.md).\n",
        );
        let files = tree(&["README.md", "docs/guide.md", "docs/setup.md", "src/main.rs"]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(found, []);
    }

    #[test]
    fn a_reference_link_resolves_through_its_definition() {
        let guide = created(
            "docs/guide.md",
            "# Guide\n\
             \n\
             Read [the setup][setup], then [faq][] and [rules].\n\
             \n\
             [setup]: setup.md#install\n\
             [faq]: <faq.md>\n\
             [rules]: ../CONTRIBUTING.md \"The rules\"\n",
        );
        let files = tree(&[
            "CONTRIBUTING.md",
            "docs/faq.md",
            "docs/guide.md",
            "docs/setup.md",
        ]);

        // Only the line with the links changes, not the definitions.
        let found = linked_files(&files, &[TouchedText::new(&guide, BTreeSet::from([3]))]);

        assert_eq!(
            found,
            [
                target("CONTRIBUTING.md", &["docs/guide.md"]),
                target("docs/faq.md", &["docs/guide.md"]),
                target("docs/setup.md", &["docs/guide.md"]),
            ]
        );
    }

    #[test]
    fn a_link_on_a_line_the_change_does_not_add_or_modify_is_ignored() {
        let guide = TouchedFile::modified(
            "docs/guide.md".to_owned(),
            "See [setup](setup.md).\nAsk in chat.\n".to_owned(),
            "See [setup](setup.md).\nSee [the faq](faq.md).\n".to_owned(),
        );
        let files = tree(&["docs/faq.md", "docs/guide.md", "docs/setup.md"]);

        let found = linked_files(&files, &[TouchedText::new(&guide, BTreeSet::from([2]))]);

        assert_eq!(found, [target("docs/faq.md", &["docs/guide.md"])]);
    }

    #[test]
    fn only_markdown_files_are_read_for_links() {
        let notes = created("docs/notes.txt", "See [setup](setup.md).\n");
        let readme = created("README.markdown", "See [faq](docs/faq.md).\n");
        let guide = created("docs/GUIDE.MD", "See [rules](../CONTRIBUTING.md).\n");
        let files = tree(&[
            "CONTRIBUTING.md",
            "README.markdown",
            "docs/GUIDE.MD",
            "docs/faq.md",
            "docs/notes.txt",
            "docs/setup.md",
        ]);

        let found = linked_files(
            &files,
            &[all_added(&notes), all_added(&readme), all_added(&guide)],
        );

        assert_eq!(
            found,
            [
                target("CONTRIBUTING.md", &["docs/GUIDE.MD"]),
                target("docs/faq.md", &["README.markdown"]),
            ]
        );
    }

    #[test]
    fn a_touched_file_is_never_a_target_and_a_target_lists_every_file_linking_to_it() {
        let usage = created(
            "docs/usage.md",
            "See [setup](setup.md) and [faq](faq.md).\n",
        );
        let faq = created("docs/faq.md", "See [setup](setup.md).\n");
        let logo = TouchedFile::created(
            "docs/logo.png".to_owned(),
            FileContent::Binary { size: 2048 },
        );
        let index = created("docs/index.md", "![The logo](logo.png)\n[Logo](logo.png)\n");
        let files = tree(&[
            "docs/faq.md",
            "docs/index.md",
            "docs/logo.png",
            "docs/setup.md",
            "docs/usage.md",
        ]);

        let found = linked_files(
            &files,
            &[
                all_added(&usage),
                all_added(&faq),
                all_added(&logo),
                all_added(&index),
            ],
        );

        assert_eq!(
            found,
            [target("docs/setup.md", &["docs/faq.md", "docs/usage.md"])]
        );
    }

    #[test]
    fn a_percent_encoded_destination_names_the_file_it_encodes() {
        let guide = created(
            "docs/guide.md",
            "See [the notes](release%20notes.md) and [the plan](<road map.md>).\n",
        );
        let files = tree(&["docs/guide.md", "docs/release notes.md", "docs/road map.md"]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(
            found,
            [
                target("docs/release notes.md", &["docs/guide.md"]),
                target("docs/road map.md", &["docs/guide.md"]),
            ]
        );
    }

    #[test]
    fn an_image_and_a_wikilink_are_not_links() {
        let guide = created(
            "docs/guide.md",
            "![The flow](flow.svg)\n\
             See [[setup]] and [[setup.md]].\n",
        );
        let files = tree(&[
            "docs/flow.svg",
            "docs/guide.md",
            "docs/setup",
            "docs/setup.md",
        ]);

        let found = linked_files(&files, &[all_added(&guide)]);

        assert_eq!(found, []);
    }
}
