//! A commit's call graph as the engine found it, and the connected code a
//! change has in it: the callers and callees of its changed callables.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::ops::RangeInclusive;
use std::path::Path;

use cgg::core::graph::CallableNode;
use cgg::{CallableId, Confidence, FileId};
use whetstone_core::Change;

use super::lines;

/// The whole call graph of a commit's analysed files: every callable with
/// its file and lines, and every call between them with its confidence.
/// File paths are relative to the repository's root, so the graph outlives
/// the directory it was found in. It holds names, paths and line numbers,
/// never file contents: the engine's signature previews and attribute
/// texts, which copy source, are dropped. It serialises, so it can be
/// cached and asked about another change at the same commit.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct Graph(cgg::Graph);

/// Shows sizes only, never a callable's name or a file's path.
impl fmt::Debug for Graph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Graph")
            .field("files", &self.0.files.len())
            .field("callables", &self.0.callables.len())
            .field("edges", &self.0.edges.len())
            .finish()
    }
}

/// A caller or callee of a changed callable, one hop away: a line range in
/// a file the change does not touch. Ranges in one file that overlap or
/// touch are one connection, with all their relations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Connection {
    path: String,
    lines: RangeInclusive<u32>,
    tier: Tier,
    relations: Vec<Relation>,
}

/// How sure the engine is of the calls behind a connection: the best
/// confidence among them. Low-confidence calls never make a connection.
/// Tiers sort best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tier {
    /// Resolved by scope, imports or receiver type.
    High,
    /// The name matches exactly one candidate in scope, but scope rules
    /// did not settle it.
    Medium,
}

/// What a connection has to do with a changed callable, which it names,
/// with the touched file the changed callable is in. Its `Debug` output
/// hides the callable's name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Relation {
    /// It calls the changed callable: it is a caller.
    Calls {
        callable: String,
        touched_file: String,
    },
    /// The changed callable calls it: it is a callee.
    CalledBy {
        callable: String,
        touched_file: String,
    },
}

impl fmt::Debug for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, touched_file) = match self {
            Self::Calls { touched_file, .. } => ("Calls", touched_file),
            Self::CalledBy { touched_file, .. } => ("CalledBy", touched_file),
        };
        f.debug_struct(kind)
            .field("touched_file", touched_file)
            .finish_non_exhaustive()
    }
}

impl Connection {
    /// The file, relative to the repository's root, with `/` between
    /// directories.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// The lines it covers in the file at the commit, 1-based and
    /// inclusive.
    pub(crate) fn lines(&self) -> RangeInclusive<u32> {
        self.lines.clone()
    }

    /// How sure the engine is of it.
    pub(crate) fn tier(&self) -> Tier {
        self.tier
    }

    /// What it has to do with the changed callables, callers' relations
    /// first, each in name order.
    pub(crate) fn relations(&self) -> &[Relation] {
        &self.relations
    }
}

/// How the other end of a call relates to its changed end, from the
/// changed callable's name and the touched file it is in.
type Relate = fn(String, String) -> Relation;

/// A callable one hop from a changed one, before ranges are merged.
#[derive(Debug)]
struct Hop {
    start: u32,
    end: u32,
    tier: Tier,
    relations: BTreeSet<Relation>,
}

impl Graph {
    /// The graph `graph` found in the directory `root`, with its files'
    /// paths made relative to `root`, and without the copies of source its
    /// callables carry.
    pub(super) fn found_in(mut graph: cgg::Graph, root: &Path) -> Self {
        for callable in graph.callables.values_mut() {
            callable.signature_hint.clear();
            callable.attributes.clear();
        }
        let relative = |path: &mut std::path::PathBuf| {
            if let Ok(inside) = path.strip_prefix(root) {
                *path = inside.to_path_buf();
            }
        };
        graph
            .files
            .values_mut()
            .for_each(|file| relative(&mut file.path));
        graph
            .file_audits
            .iter_mut()
            .for_each(|audit| relative(&mut audit.path));
        Self(graph)
    }

    /// How many files, callables and calls the graph holds.
    pub(super) fn sizes(&self) -> (usize, usize, usize) {
        (
            self.0.files.len(),
            self.0.callables.len(),
            self.0.edges.len(),
        )
    }

    /// The connected code of `change` in this graph, which must be of the
    /// change's commit: the callers and callees of every changed callable,
    /// in path and line order.
    ///
    /// A changed callable is one in a file the change leaves at the commit
    /// whose lines include a line the change adds or modifies, or lines the
    /// change removes, as [`Touched::touches`](lines::Touched::touches)
    /// decides. A function the change deletes is not in the graph, so it
    /// has no connected code.
    ///
    /// Callables with no lines of their own, which the engine makes up for
    /// a framework's entry point, are never changed or connected. Those it
    /// marks synthetic but places in the file, such as an inline route
    /// handler in JavaScript, are, since they are real code. Calls of low
    /// confidence are left out, and so is any callable in a file the change
    /// touches, which is sent already.
    pub(crate) fn connections(&self, change: &Change) -> Vec<Connection> {
        let touched = lines::touched(change);
        let paths: HashMap<FileId, &str> = self
            .0
            .files
            .iter()
            .filter_map(|(id, file)| Some((*id, file.path.to_str()?)))
            .collect();
        let placed = |id: &CallableId| {
            let callable = self.0.callables.get(id)?;
            let path = paths.get(&callable.file)?;
            (callable.start_line > 0).then_some((callable, *path))
        };
        let changed: HashSet<CallableId> = self
            .0
            .callables
            .keys()
            .filter(|id| {
                placed(id).is_some_and(|(callable, path)| {
                    touched.get(path).is_some_and(|touched| {
                        touched.touches(callable.start_line, callable.end_line)
                    })
                })
            })
            .copied()
            .collect();
        let mut hops: BTreeMap<&str, Vec<Hop>> = BTreeMap::new();
        for edge in &self.0.edges {
            let tier = match edge.confidence {
                Confidence::High => Tier::High,
                Confidence::Medium => Tier::Medium,
                Confidence::Low => continue,
            };
            let ends: [(_, _, Relate); 2] = [
                (edge.dst, edge.src, |callable, touched_file| {
                    Relation::Calls {
                        callable,
                        touched_file,
                    }
                }),
                (edge.src, edge.dst, |callable, touched_file| {
                    Relation::CalledBy {
                        callable,
                        touched_file,
                    }
                }),
            ];
            for (changed_end, other_end, relation) in ends {
                if !changed.contains(&changed_end) {
                    continue;
                }
                let (Some((changed_callable, changed_path)), Some((other, path))) =
                    (placed(&changed_end), placed(&other_end))
                else {
                    continue;
                };
                if touched.contains_key(path) {
                    continue;
                }
                hops.entry(path).or_default().push(Hop {
                    start: other.start_line,
                    end: other.end_line.max(other.start_line),
                    tier,
                    relations: BTreeSet::from([relation(
                        name(changed_callable),
                        changed_path.to_owned(),
                    )]),
                });
            }
        }
        hops.into_iter()
            .flat_map(|(path, hops)| {
                merged(hops).into_iter().map(move |hop| Connection {
                    path: path.to_owned(),
                    lines: hop.start..=hop.end,
                    tier: hop.tier,
                    relations: hop.relations.into_iter().collect(),
                })
            })
            .collect()
    }
}

/// How a relation names a changed callable: by its own name, as `charge`
/// for the method `Payments::charge`, without its owner or module path.
fn name(callable: &CallableNode) -> String {
    callable.simple_name.clone()
}

/// `hops` in one file, with those that overlap or touch merged: a merged
/// range keeps every relation and the best tier.
fn merged(mut hops: Vec<Hop>) -> Vec<Hop> {
    hops.sort_by_key(|hop| (hop.start, hop.end));
    let mut merged: Vec<Hop> = Vec::with_capacity(hops.len());
    for hop in hops {
        match merged.last_mut() {
            Some(last) if hop.start <= last.end.saturating_add(1) => {
                last.end = last.end.max(hop.end);
                last.tier = last.tier.min(hop.tier);
                last.relations.extend(hop.relations);
            }
            _ => merged.push(hop),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn the_graph_keeps_paths_relative_to_the_repository_and_no_copies_of_source() {
        let root = TempDir::new().expect("create a temporary directory");
        fs::create_dir(root.path().join("app")).expect("create a directory");
        fs::write(
            root.path().join("app/routes.py"),
            "@app.route(\"/secret-admin\")\ndef index(token=\"hunter2\"):\n    return token\n",
        )
        .expect("write a file");
        let options = cgg::RunOptions::new(vec![root.path().to_path_buf()]);
        let graph = cgg::analyze(&options).expect("analyse the file").graph;
        let has_source = |graph: &cgg::Graph| {
            graph.callables.values().any(|callable| {
                !callable.signature_hint.is_empty() || !callable.attributes.is_empty()
            })
        };
        assert!(has_source(&graph), "the engine copies source");

        let Graph(graph) = Graph::found_in(graph, root.path());

        assert!(!has_source(&graph));
        let paths: Vec<_> = graph.files.values().map(|file| file.path.clone()).collect();
        assert!(
            paths.contains(&Path::new("app/routes.py").to_path_buf()),
            "{paths:?}"
        );
    }
}
