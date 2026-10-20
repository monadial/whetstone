//! A commit's call graph as the engine found it, and the connected code a
//! change has in it: the callers and callees of its changed callables.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "wired into the outbound review by the #58 integration"
    )
)]

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
/// the directory it was found in, and it holds names, paths and line
/// numbers, never file contents. It serialises, so it can be cached and
/// asked about another change at the same commit.
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

/// What a connection has to do with a changed callable, which it names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Relation {
    /// It calls the changed callable: it is a caller.
    Calls(String),
    /// The changed callable calls it: it is a callee.
    CalledBy(String),
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
    /// paths made relative to `root`.
    pub(super) fn found_in(mut graph: cgg::Graph, root: &Path) -> Self {
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
    /// decides. Callables the engine made up, with no lines of their own,
    /// are never changed or connected. A function the change
    /// deletes is not in the graph, so it has no connected code. Calls of
    /// low confidence are left out, and so is any callable in a file the
    /// change touches, which is sent already.
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
            (!callable.synthetic && callable.start_line > 0).then_some((callable, *path))
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
            let ends = [
                (
                    edge.dst,
                    edge.src,
                    Relation::Calls as fn(String) -> Relation,
                ),
                (edge.src, edge.dst, Relation::CalledBy),
            ];
            for (changed_end, other_end, relation) in ends {
                if !changed.contains(&changed_end) {
                    continue;
                }
                let (Some((changed_callable, _)), Some((other, path))) =
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
                    relations: BTreeSet::from([relation(name(changed_callable))]),
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
