//! Which of a commit's files `cgg` reads, decided from each file's path and
//! contents before anything is written, so only those are written for it.

use std::path::{Component, Path, PathBuf};

use cgg_lang::PluginRegistry;

/// What `cgg` reads, from its own language registry and walker settings.
#[derive(Debug)]
pub(super) struct Engine {
    registry: PluginRegistry,
    /// The largest file its walker reads, in bytes, if it has a limit.
    max_file_size: Option<u64>,
}

impl Engine {
    pub(super) fn new() -> Self {
        Self {
            registry: PluginRegistry::with_v1_plugins(),
            max_file_size: cgg_walk::WalkConfig::default().max_file_size,
        }
    }

    /// The first directory in `path` that `cgg`'s walker never reads in,
    /// such as `target` or `node_modules`. It checks every directory of a
    /// file's path, the analysed directory's ancestors included.
    pub(super) fn denied_directory(path: &Path) -> Option<&'static str> {
        path.components().find_map(|component| match component {
            Component::Normal(name) => cgg_walk::BUILTIN_DENY_DIRS
                .iter()
                .find(|denied| name == **denied)
                .copied(),
            _ => None,
        })
    }

    /// Whether `cgg`'s walker reads a file of `size` bytes.
    pub(super) fn reads_size(&self, size: usize) -> bool {
        self.max_file_size
            .is_none_or(|max| u64::try_from(size).is_ok_and(|size| size <= max))
    }

    /// Whether `cgg`'s language detector recognises the file at `path`
    /// holding `contents`. Its rules, in order: a shebang naming a
    /// language; for YAML and JSON, an API description (`openapi`,
    /// `swagger` or `asyncapi` at its root) and nothing else; then the
    /// extension.
    pub(super) fn recognises(&self, path: &str, contents: &[u8]) -> bool {
        if let Some(line) = shebang(contents) {
            let names_a_language = self.registry.all().iter().any(|plugin| {
                plugin
                    .shebangs()
                    .iter()
                    .any(|keyword| line.contains(keyword))
            });
            if names_a_language {
                return true;
            }
        }
        let Some(extension) = Path::new(path).extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        let extension = format!(".{extension}");
        let lower = extension.to_ascii_lowercase();
        if matches!(lower.as_str(), ".yaml" | ".yml" | ".json") {
            return is_api_descriptor(contents);
        }
        self.registry.all().iter().any(|plugin| {
            plugin
                .extensions()
                .iter()
                .any(|known| *known == extension || *known == lower)
        })
    }
}

/// The repository path `path` as a path relative to the directory it is
/// written into, or `None` if it could leave that directory or name it:
/// an absolute path, `.`, `..` or an empty component.
pub(super) fn relative_path(path: &str) -> Option<PathBuf> {
    path.split('/')
        .map(|component| {
            (!matches!(component, "" | "." | "..") && !component.contains('\0'))
                .then_some(component)
        })
        .collect()
}

/// The first line of `contents` if they start with a shebang, as `cgg`
/// reads it: from at most the first 256 bytes, and only if it is UTF-8.
fn shebang(contents: &[u8]) -> Option<&str> {
    let head = &contents[..contents.len().min(256)];
    if !head.starts_with(b"#!") {
        return None;
    }
    let end = head
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(head.len());
    std::str::from_utf8(&head[..end]).ok()
}

/// Whether `contents` start like an API description, as `cgg` sniffs YAML
/// and JSON: `openapi`, `swagger` or `asyncapi` as a key at the start of
/// one of the first 64 lines, or quoted near the start of a JSON object.
fn is_api_descriptor(contents: &[u8]) -> bool {
    const KEYS: [&str; 3] = ["openapi", "swagger", "asyncapi"];
    let head = String::from_utf8_lossy(&contents[..contents.len().min(8192)]);
    let keyed_line = head.lines().take(64).any(|line| {
        let line = line.trim_start().trim_start_matches(['"', '\'']);
        KEYS.iter().any(|key| {
            line.strip_prefix(key).is_some_and(|rest| {
                rest.trim_start_matches(['"', '\''])
                    .trim_start()
                    .starts_with(':')
            })
        })
    });
    if keyed_line {
        return true;
    }
    if !head.trim_start().starts_with('{') {
        return false;
    }
    let mut end = head.len().min(2048);
    while !head.is_char_boundary(end) {
        end -= 1;
    }
    let window = head.get(..end).unwrap_or_default();
    KEYS.iter()
        .any(|key| window.contains(&format!("\"{key}\"")))
}
