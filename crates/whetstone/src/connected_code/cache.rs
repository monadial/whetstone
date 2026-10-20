//! The graph cache: each analysed commit's graph, kept on the learner's
//! machine so that going back, retrying or practising on it again does not
//! analyse it again.

use std::fs::{DirBuilder, File};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Instant, SystemTime};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Whetstone's cache format version. Bump it whenever the way a graph is
/// stored changes, so entries written the old way are never read. A
/// commit's key does not hold Whetstone's version, so bump it too whenever
/// what goes into a graph changes, such as which files are written for the
/// engine or what the graph drops.
const FORMAT: u32 = 1;

/// The on-disk store of connected-code graphs, one compressed JSON file per
/// [`GraphKey`] in a `graphs` folder inside the cache folder.
///
/// A graph's type is the caller's: any value serde can serialise. The
/// caller's contract is that it holds only names, paths and line numbers,
/// never file contents, since the cache keeps it on disk.
///
/// Its calls block on file I/O. The app opens it once, at launch, and gets
/// and puts graphs from a blocking job. None of them fails the caller:
/// anything that cannot be read is a miss, and a graph that cannot be
/// written is not cached. The first time the cache folder cannot be
/// written, a warning is logged that names the cache folder and no path
/// inside it.
#[derive(Debug)]
pub(crate) struct GraphCache {
    /// Whetstone's cache folder.
    folder: PathBuf,
    /// The `graphs` folder inside it.
    graphs: PathBuf,
    /// The most the entries may hold in total, in bytes.
    cap: u64,
    /// Logs that the cache folder cannot be written, once.
    unwritable: Once,
}

impl GraphCache {
    /// The most the cache holds on disk: 256 MB.
    pub(crate) const CAP: u64 = 256 * 1000 * 1000;

    /// Opens the store in `folder`, Whetstone's cache folder, creating its
    /// `graphs` folder, mode 0700, if needed. The entries may hold at most
    /// `cap` bytes in total.
    pub(crate) fn open(folder: &Path, cap: u64) -> Self {
        let cache = Self {
            folder: folder.to_owned(),
            graphs: folder.join("graphs"),
            cap,
            unwritable: Once::new(),
        };
        if let Err(error) = DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&cache.graphs)
        {
            cache.warn_unwritable(&error);
        }
        cache
    }

    /// The graph stored under `key`, if there is one. A hit makes it the
    /// most recently used graph. An entry that cannot be read, or does not
    /// hold a `T`, is a miss, and is removed so the caller's rebuilt graph
    /// replaces it.
    pub(crate) fn get<T: DeserializeOwned>(&self, key: &GraphKey) -> Option<T> {
        let started = Instant::now();
        let path = self.graphs.join(key.file_name());
        match read(&path) {
            Ok((graph, bytes)) => {
                tracing::info!(
                    bytes,
                    ms = started.elapsed().as_millis(),
                    "loaded a cached graph"
                );
                Some(graph)
            }
            Err(Miss::Absent) => None,
            Err(Miss::Damaged) => {
                tracing::info!("a cached graph cannot be read; removing it");
                // If it cannot be removed, the rebuilt graph's write replaces
                // it.
                let _removed = std::fs::remove_file(path);
                None
            }
        }
    }

    /// Stores `graph` under `key`, replacing what was there, then removes
    /// the least recently used graphs until the rest fit in the cap.
    pub(crate) fn put<T: Serialize>(&self, key: &GraphKey, graph: &T) {
        let started = Instant::now();
        let json = match serde_json::to_vec(graph) {
            Ok(json) => json,
            Err(error) => {
                // Only the category: the message may quote the graph.
                tracing::warn!(category = ?error.classify(), "cannot encode a graph for the cache");
                return;
            }
        };
        match self
            .write(key, &json)
            .and_then(|bytes| Ok((bytes, self.evict()?)))
        {
            Ok((bytes, removed)) => tracing::info!(
                bytes,
                removed,
                ms = started.elapsed().as_millis(),
                "cached a graph"
            ),
            Err(error) => self.warn_unwritable(&error),
        }
    }

    /// Logs, the first time only, that the cache folder cannot be written.
    /// Only the error's kind is logged: the error itself may name a path
    /// inside the folder.
    fn warn_unwritable(&self, error: &io::Error) {
        self.unwritable.call_once(|| {
            tracing::warn!(
                folder = %self.folder.display(),
                reason = %error.kind(),
                "cannot write to the cache folder; carrying on without caching graphs"
            );
        });
    }

    /// Writes `json` compressed to a temporary file in the `graphs` folder,
    /// then renames it to `key`'s file, so a crash never leaves a
    /// half-written graph, and gives the file's size. A temporary file that
    /// a failed write leaves is removed when it drops.
    fn write(&self, key: &GraphKey, json: &[u8]) -> io::Result<u64> {
        let mut file = tempfile::NamedTempFile::new_in(&self.graphs)?;
        let mut encoder = GzEncoder::new(&mut file, Compression::default());
        encoder.write_all(json)?;
        encoder.finish()?;
        // Stamped rather than left to the file system, whose clock can be
        // too coarse to tell two writes in a row apart.
        file.as_file().set_modified(SystemTime::now())?;
        let bytes = file.as_file().metadata()?.len();
        file.persist(self.graphs.join(key.file_name()))?;
        Ok(bytes)
    }

    /// Removes the least recently used files in the `graphs` folder until
    /// the rest fit in the cap. A file's last use is its modified time.
    /// Every file counts, so one left by an older format or engine version,
    /// or a temporary file left by a crash, is removed in its turn. A file
    /// that goes meanwhile, because another Whetstone shares the folder, is
    /// not an error. Gives how many files it removed.
    fn evict(&self) -> io::Result<usize> {
        let mut files: Vec<(SystemTime, u64, PathBuf)> = std::fs::read_dir(&self.graphs)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let metadata = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
                Some((metadata.modified().ok()?, metadata.len(), entry.path()))
            })
            .collect();
        let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
        // Oldest last use first.
        files.sort();
        let mut removed = 0;
        for (_, size, path) in files {
            if total <= self.cap {
                break;
            }
            match std::fs::remove_file(path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            total -= size;
        }
        Ok(removed)
    }
}

/// Which tree a [`Snapshot`](super::Snapshot) shows, which names its graph
/// in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotId {
    /// A commit's tree in the learner's repository, by its ID, which every
    /// commit with the same files shares.
    Tree(gix::ObjectId),
    /// The tree at a showcase change, by the change's position in the
    /// showcase project this Whetstone version ships.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the showcase project's snapshot arrives with #65")
    )]
    Showcase(usize),
}

/// What one cached graph was built from, by which engine version, stored in
/// which format. Each key has its own file, named after the whole key, so a
/// graph from another engine or format version never matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphKey {
    source: Source,
    engine: String,
    format: u32,
}

/// What was analysed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// A commit in the learner's repository, by its tree.
    Tree(gix::ObjectId),
    /// A showcase change, by the Whetstone version that ships it and its
    /// position in the showcase project.
    Showcase {
        whetstone: &'static str,
        position: usize,
    },
}

impl GraphKey {
    /// The key for the graph of the tree `snapshot` names, found by version
    /// `engine` of the engine.
    pub(crate) fn of(snapshot: SnapshotId, engine: &str) -> Self {
        match snapshot {
            SnapshotId::Tree(tree) => Self::tree(tree, engine),
            SnapshotId::Showcase(position) => Self::showcase(position, engine),
        }
    }

    /// The key for a commit's graph: the commit's tree ID, and the version of
    /// the engine that analyses it.
    pub(crate) fn tree(tree: gix::ObjectId, engine: &str) -> Self {
        Self {
            source: Source::Tree(tree),
            engine: engine.to_owned(),
            format: FORMAT,
        }
    }

    /// The key for a showcase change's graph: the change's position in the
    /// showcase project, which this Whetstone version ships, and the version
    /// of the engine that analyses it.
    pub(crate) fn showcase(position: usize, engine: &str) -> Self {
        Self {
            source: Source::Showcase {
                whetstone: env!("CARGO_PKG_VERSION"),
                position,
            },
            engine: engine.to_owned(),
            format: FORMAT,
        }
    }

    /// The entry's file name, derived from every part of the key.
    fn file_name(&self) -> String {
        let source = match &self.source {
            Source::Tree(tree) => format!("tree-{tree}"),
            Source::Showcase {
                whetstone,
                position,
            } => format!("showcase-{}-{position}", escape(whetstone)),
        };
        format!(
            "v{}-{source}-engine-{}.json.gz",
            self.format,
            escape(&self.engine)
        )
    }
}

#[cfg(test)]
impl GraphKey {
    /// The same key in another cache format, as another Whetstone release
    /// makes it.
    pub(super) fn in_another_format(self) -> Self {
        Self {
            format: FORMAT + 1,
            ..self
        }
    }
}

/// `text` with every byte other than an ASCII letter, a digit or `.` written
/// as `_` and two hex digits, so it is safe in a file name, never contains
/// the `-` that separates a key's parts, and two texts never escape alike.
fn escape(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || byte == b'.' {
                char::from(byte).to_string()
            } else {
                format!("_{byte:02x}")
            }
        })
        .collect()
}

/// Why an entry gives no graph.
#[derive(Debug)]
enum Miss {
    /// There is no entry.
    Absent,
    /// The entry cannot be opened, decompressed or decoded.
    Damaged,
}

/// The graph in the entry at `path`, which then becomes the most recently
/// used, and the entry's size.
fn read<T: DeserializeOwned>(path: &Path) -> Result<(T, u64), Miss> {
    let file = File::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Miss::Absent,
        _ => Miss::Damaged,
    })?;
    let bytes = file.metadata().map_err(|_| Miss::Damaged)?.len();
    let mut json = Vec::new();
    GzDecoder::new(&file)
        .read_to_end(&mut json)
        .map_err(|_| Miss::Damaged)?;
    let graph = serde_json::from_slice(&json).map_err(|_| Miss::Damaged)?;
    // A hit stands even if its last use cannot be refreshed: the entry may
    // then be removed sooner than it should be, and is rebuilt.
    let _refreshed = file.set_modified(SystemTime::now());
    Ok((graph, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::{Arc, Mutex};

    use serde::{Deserialize, Serialize};

    /// A stand-in for the engine's graph: names, paths and line numbers.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Graph {
        edges: Vec<(String, String, u32)>,
    }

    fn graph(caller: &str) -> Graph {
        Graph {
            edges: vec![(caller.to_owned(), "src/lib.rs".to_owned(), 42)],
        }
    }

    fn tree(hex: &str) -> gix::ObjectId {
        gix::ObjectId::from_hex(hex.repeat(40).as_bytes())
            .expect("the test tree ID is 40 hex digits")
    }

    /// The size on disk of one test graph's entry, measured in a cache of
    /// its own. Every test graph's caller name has the same length, so all
    /// their entries are about this size.
    fn entry_size() -> u64 {
        let folder = tempfile::tempdir().expect("a temporary folder");
        GraphCache::open(folder.path(), GraphCache::CAP)
            .put(&GraphKey::tree(tree("f"), "0.11.0"), &graph("first"));
        files(folder.path())
            .iter()
            .map(|file| file.metadata().expect("the entry has metadata").len())
            .sum()
    }

    /// Room for two test graphs, but not three.
    fn room_for_two_graphs() -> u64 {
        entry_size() * 5 / 2
    }

    /// Every file in the cache folder's `graphs` folder.
    fn files(folder: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(folder.join("graphs"))
            .expect("the graphs folder can be listed")
            .map(|entry| entry.expect("a folder entry").path())
            .collect()
    }

    /// A cache folder that cannot be written: a file stands where it
    /// should be, which stops even a privileged user.
    fn unwritable_folder(parent: &Path) -> PathBuf {
        let folder = parent.join("cache");
        std::fs::write(&folder, "").expect("a file where the cache folder should be");
        folder
    }

    /// Everything logged while `run` runs.
    fn logged(run: impl FnOnce()) -> String {
        #[derive(Debug, Clone, Default)]
        struct Captured(Arc<Mutex<Vec<u8>>>);

        impl io::Write for Captured {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.lock().expect("log lock").extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        let bytes = captured.0.lock().expect("log lock").clone();
        String::from_utf8(bytes).expect("the log is UTF-8")
    }

    #[test]
    fn a_graph_put_in_the_cache_comes_back() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        let key = GraphKey::tree(tree("a"), "0.11.0");

        cache.put(&key, &graph("parse"));

        assert_eq!(cache.get::<Graph>(&key), Some(graph("parse")));
    }

    #[test]
    fn the_least_recently_used_graph_goes_first() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), room_for_two_graphs());
        let (first, second, third) = (
            GraphKey::tree(tree("1"), "0.11.0"),
            GraphKey::tree(tree("2"), "0.11.0"),
            GraphKey::tree(tree("3"), "0.11.0"),
        );

        cache.put(&first, &graph("first"));
        cache.put(&second, &graph("secnd"));
        cache.put(&third, &graph("third"));

        assert_eq!(cache.get::<Graph>(&first), None);
        assert_eq!(cache.get::<Graph>(&second), Some(graph("secnd")));
        assert_eq!(cache.get::<Graph>(&third), Some(graph("third")));
        assert_eq!(files(folder.path()).len(), 2);
    }

    #[test]
    fn a_hit_makes_its_graph_the_most_recently_used() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), room_for_two_graphs());
        let (first, second, third) = (
            GraphKey::tree(tree("1"), "0.11.0"),
            GraphKey::tree(tree("2"), "0.11.0"),
            GraphKey::tree(tree("3"), "0.11.0"),
        );
        cache.put(&first, &graph("first"));
        cache.put(&second, &graph("secnd"));

        assert_eq!(cache.get::<Graph>(&first), Some(graph("first")));
        cache.put(&third, &graph("third"));

        assert_eq!(cache.get::<Graph>(&second), None);
        assert_eq!(cache.get::<Graph>(&first), Some(graph("first")));
        assert_eq!(cache.get::<Graph>(&third), Some(graph("third")));
    }

    #[test]
    fn a_graph_saved_by_another_engine_version_is_never_used() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        cache.put(&GraphKey::tree(tree("a"), "0.11.0"), &graph("older"));

        assert_eq!(
            cache.get::<Graph>(&GraphKey::tree(tree("a"), "0.12.0")),
            None
        );
        assert_eq!(cache.get::<Graph>(&GraphKey::tree(tree("a"), "0.11")), None);
    }

    #[test]
    fn a_graph_saved_in_another_cache_format_is_never_used() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        let current = GraphKey::tree(tree("a"), "0.11.0");
        let other_release = GraphKey {
            format: FORMAT + 1,
            ..current.clone()
        };
        cache.put(&other_release, &graph("older"));

        assert_eq!(cache.get::<Graph>(&current), None);
    }

    #[test]
    fn each_showcase_change_has_its_own_graph() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        cache.put(&GraphKey::showcase(0, "0.11.0"), &graph("first"));
        cache.put(&GraphKey::showcase(1, "0.11.0"), &graph("secnd"));

        assert_eq!(
            cache.get::<Graph>(&GraphKey::showcase(0, "0.11.0")),
            Some(graph("first"))
        );
        assert_eq!(
            cache.get::<Graph>(&GraphKey::showcase(1, "0.11.0")),
            Some(graph("secnd"))
        );
        assert_eq!(cache.get::<Graph>(&GraphKey::showcase(0, "0.12.0")), None);
    }

    #[test]
    fn a_damaged_graph_is_a_miss_and_is_removed() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        let key = GraphKey::tree(tree("a"), "0.11.0");
        cache.put(&key, &graph("parse"));
        let [entry] = files(folder.path()).try_into().expect("one entry");
        let stored = std::fs::read(&entry).expect("the entry can be read");
        std::fs::write(&entry, &stored[..stored.len() / 2]).expect("the entry can be cut short");

        assert_eq!(cache.get::<Graph>(&key), None);
        assert_eq!(files(folder.path()), Vec::<PathBuf>::new());

        cache.put(&key, &graph("parse"));
        assert_eq!(cache.get::<Graph>(&key), Some(graph("parse")));
    }

    #[test]
    fn a_graph_of_another_shape_is_a_miss_and_is_removed() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        let key = GraphKey::tree(tree("a"), "0.11.0");
        cache.put(&key, &"not a graph");

        assert_eq!(cache.get::<Graph>(&key), None);
        assert_eq!(files(folder.path()), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_unwritable_cache_folder_caches_nothing_and_fails_nothing() {
        let parent = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(&unwritable_folder(parent.path()), GraphCache::CAP);
        let key = GraphKey::tree(tree("a"), "0.11.0");

        cache.put(&key, &graph("parse"));

        assert_eq!(cache.get::<Graph>(&key), None);
    }

    #[test]
    fn an_unwritable_cache_folder_is_logged_once_naming_no_path_inside_it() {
        let parent = tempfile::tempdir().expect("a temporary folder");
        let folder = unwritable_folder(parent.path());

        let log = logged(|| {
            let cache = GraphCache::open(&folder, GraphCache::CAP);
            cache.put(&GraphKey::tree(tree("a"), "0.11.0"), &graph("parse"));
            cache.put(&GraphKey::tree(tree("b"), "0.11.0"), &graph("parse"));
        });

        assert_eq!(log.matches("WARN").count(), 1, "{log}");
        assert!(log.contains(&folder.display().to_string()), "{log}");
        let inside = format!("{}/", folder.display());
        for unlogged in [inside.as_str(), ".json.gz", "parse"] {
            assert!(!log.contains(unlogged), "{unlogged} is logged: {log}");
        }
    }

    #[test]
    fn only_the_learner_can_open_the_graphs_folder() {
        let folder = tempfile::tempdir().expect("a temporary folder");

        GraphCache::open(folder.path(), GraphCache::CAP);

        let graphs =
            std::fs::metadata(folder.path().join("graphs")).expect("the graphs folder exists");
        assert!(graphs.is_dir());
        assert_eq!(graphs.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn a_write_leaves_no_temporary_file_behind() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        let key = GraphKey::tree(tree("a"), "0.11.0");

        cache.put(&key, &graph("first"));
        cache.put(&key, &graph("secnd"));

        assert_eq!(files(folder.path()).len(), 1);
        assert_eq!(cache.get::<Graph>(&key), Some(graph("secnd")));
    }

    #[test]
    fn a_graph_that_cannot_be_encoded_is_not_cached_and_fails_nothing() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cache = GraphCache::open(folder.path(), GraphCache::CAP);
        let key = GraphKey::tree(tree("a"), "0.11.0");
        let unencodable = BTreeMap::from([(vec![1_u8], 2_u8)]);

        let log = logged(|| cache.put(&key, &unencodable));

        assert_eq!(cache.get::<BTreeMap<Vec<u8>, u8>>(&key), None);
        assert_eq!(files(folder.path()), Vec::<PathBuf>::new());
        assert_eq!(log.matches("WARN").count(), 1, "{log}");
    }
}
