//! The evidence as sent: the one JSON value that is Jev's state, or part of
//! it, and the generator's input, or part of it.
//!
//! It holds only what the learner approved in the outbound review: what
//! [`Evidence`]'s public interface says is sent. A left-out file or
//! connection sends nothing, not even its path. Area prompts are never part
//! of it: the generator gets its area's prompt beside it in its input, and
//! Jev never gets one.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use whetstone_core::{ConnectionGroup, Evidence, Relation, Side};

/// `evidence` as it is sent: the commit message; each included touched
/// file's path, diff and full content; each included connection's path,
/// relations and lines, by group; and the included touched files that come
/// with no connected code.
///
/// Every line of content is numbered as in its file, as `12: text`, so that
/// a citation can name the lines it rests on.
pub(crate) fn evidence_as_sent(evidence: &Evidence) -> Value {
    let files: Vec<_> = evidence
        .files()
        .iter()
        .filter_map(|file| file.sent().map(|sent| (file, sent)))
        .collect();
    let sent_paths: BTreeSet<&str> = files.iter().map(|(file, _)| file.path()).collect();
    let touched_files: Vec<Value> = files
        .iter()
        .map(|(file, sent)| {
            let mut touched = json!({ "path": file.path() });
            if let Some(from) = file.renamed_from() {
                touched["renamed_from"] = json!(from);
            }
            touched["diff"] = json!(sent.diff);
            let content = match sent.side {
                Side::After => "content_after_change",
                Side::Before => "content_before_change",
            };
            touched[content] = json!(numbered(sent.content, 1));
            touched
        })
        .collect();
    let mut connected_code = Vec::new();
    let mut related_tests = Vec::new();
    let mut related_configuration = Vec::new();
    let mut connected_files = BTreeSet::new();
    for connection in evidence.connections() {
        let Some(sent) = connection.sent() else {
            continue;
        };
        // A relation to a touched file that is left out would name it.
        let relations: Vec<&Relation> = connection
            .relations()
            .iter()
            .filter(|relation| sent_paths.contains(relation.touched_file()))
            .collect();
        let group = match connection.group() {
            ConnectionGroup::ConnectedCode => {
                connected_files.extend(relations.iter().map(|relation| relation.touched_file()));
                &mut connected_code
            }
            ConnectionGroup::RelatedTest => &mut related_tests,
            ConnectionGroup::RelatedConfiguration => &mut related_configuration,
        };
        group.push(json!({
            "path": connection.id().path(),
            "relations": relations.into_iter().map(described).collect::<Vec<_>>(),
            "lines": numbered(sent.text, sent.first_line),
        }));
    }
    let without_connected_code: Vec<&str> = files
        .iter()
        .map(|(file, _)| file.path())
        .filter(|path| !connected_files.contains(path))
        .collect();
    json!({
        "commit_message": evidence.message(),
        "touched_files": touched_files,
        "connected_code": connected_code,
        "related_tests": related_tests,
        "related_configuration": related_configuration,
        "files_without_connected_code": without_connected_code,
    })
}

/// `text`'s lines, each after its number, counting from `first`.
fn numbered(text: &str, first: usize) -> String {
    text.lines()
        .enumerate()
        .map(|(index, line)| format!("{}: {line}", first + index))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How a connection connects to a touched file, naming the touched file.
fn described(relation: &Relation) -> String {
    match relation {
        // "calls `charge`" names the callable, not the file it is in.
        Relation::Calls { touched_file, .. } | Relation::CalledBy { touched_file, .. } => {
            format!("{relation}, changed in {touched_file}")
        }
        Relation::LinkedFrom { .. }
        | Relation::NamedIn { .. }
        | Relation::TestFor { .. }
        | Relation::ManifestFor { .. } => relation.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use whetstone_core::{
        Change, ConnectedCodeLimit, Connection, Connections, ExcludePatterns, Extent,
        ReviewChoices, Tier, TouchedFile,
    };

    use super::*;
    use crate::candidates_request;
    use crate::generator::GeneratorRequest;
    use crate::samples::{area, evidence, generator_model};

    /// The request that writes candidates from `evidence`, whose input
    /// holds the evidence as sent, as `evidence`.
    fn request(evidence: &Evidence) -> GeneratorRequest {
        candidates_request(&generator_model(), evidence, &area(), NonZeroUsize::MIN)
    }

    /// The evidence as sent, as `evidence`'s request's input holds it.
    fn sent(evidence: &Evidence) -> Value {
        serde_json::from_str::<Value>(&request(evidence).input).expect("the input is JSON")
            ["evidence"]
            .take()
    }

    /// Evidence for `files`, whose diffs are `diff`, with `connections`.
    fn build(files: Vec<TouchedFile>, diff: &str, connections: &Connections) -> Evidence {
        Evidence::build(
            &Change::new("Tidy up".to_owned(), Vec::new(), diff.to_owned(), files),
            &ExcludePatterns::default(),
            &[],
            connections,
            ConnectedCodeLimit::effective(None, None),
            &ReviewChoices::default(),
        )
    }

    #[test]
    fn a_file_left_out_sends_nothing_not_even_its_path() {
        let input = request(&evidence()).input;

        assert!(!input.contains("Cargo.lock"));
        assert!(!input.contains("version = "));
    }

    #[test]
    fn the_area_goes_in_the_input_beside_the_evidence_and_never_in_the_preamble() {
        let request = request(&evidence());
        let input: Value = serde_json::from_str(&request.input).expect("the input is JSON");

        assert_eq!(
            input["area"],
            json!({
                "name": "Domain",
                "description": "The rules, concepts and language the work is about",
                "prompt": "Focus on the rules the change encodes: what must always hold, and what\n\
                           would go wrong if it didn't.",
            })
        );
        for text in [
            "Domain",
            "The rules, concepts and language the work is about",
            "Focus on the rules the change encodes",
        ] {
            assert!(
                !request.preamble.contains(text),
                "{text:?} is in the preamble"
            );
            assert!(
                !input["evidence"].to_string().contains(text),
                "{text:?} is in the evidence"
            );
        }
    }

    #[test]
    fn a_deleted_file_sends_its_content_before_the_change_and_a_renamed_one_its_old_path() {
        let evidence = build(
            vec![
                TouchedFile::deleted("src/old.rs".to_owned(), "gone\n".to_owned()),
                TouchedFile::renamed(
                    "src/legacy.rs".to_owned(),
                    "src/new.rs".to_owned(),
                    "kept\n".to_owned(),
                    "kept\nadded\n".to_owned(),
                ),
            ],
            "diff --git a/src/old.rs b/src/old.rs\n-gone\n\
             diff --git a/src/legacy.rs b/src/new.rs\n+added\n",
            &Connections::NoneFound,
        );

        let sent = sent(&evidence);

        assert_eq!(
            sent["touched_files"],
            json!([
                {
                    "path": "src/old.rs",
                    "diff": "diff --git a/src/old.rs b/src/old.rs\n-gone\n",
                    "content_before_change": "1: gone",
                },
                {
                    "path": "src/new.rs",
                    "renamed_from": "src/legacy.rs",
                    "diff": "diff --git a/src/legacy.rs b/src/new.rs\n+added\n",
                    "content_after_change": "1: kept\n2: added",
                },
            ])
        );
        assert_eq!(
            sent["files_without_connected_code"],
            json!(["src/old.rs", "src/new.rs"])
        );
    }

    #[test]
    fn a_relation_to_a_file_left_out_is_not_sent() {
        let evidence = build(
            vec![
                TouchedFile::modified(
                    "src/config.rs".to_owned(),
                    "fn load() {}\n".to_owned(),
                    "fn load() { read() }\n".to_owned(),
                ),
                TouchedFile::modified(
                    ".env".to_owned(),
                    "TOKEN=a\n".to_owned(),
                    "TOKEN=b\n".to_owned(),
                ),
            ],
            "diff --git a/src/config.rs b/src/config.rs\n+fn load() { read() }\n\
             diff --git a/.env b/.env\n+TOKEN=b\n",
            &Connections::Found(vec![Connection::new(
                "src/read.rs".to_owned(),
                Extent::WholeFile,
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![
                    Relation::CalledBy {
                        callable: "load".to_owned(),
                        touched_file: "src/config.rs".to_owned(),
                    },
                    Relation::NamedIn {
                        touched_file: ".env".to_owned(),
                    },
                ],
                "fn read() {}\n".to_owned().into(),
            )]),
        );

        let input = request(&evidence).input;

        assert_eq!(
            serde_json::from_str::<Value>(&input).expect("the input is JSON")["evidence"]["connected_code"],
            json!([{
                "path": "src/read.rs",
                "relations": ["called by `load`, changed in src/config.rs"],
                "lines": "1: fn read() {}",
            }])
        );
        assert!(!input.contains(".env"));
    }
}
