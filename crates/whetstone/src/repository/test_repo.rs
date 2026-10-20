//! Throwaway Git repositories with real commits, for tests of reading
//! commits and finding their connected code. Like the integration tests'
//! `TestRepo`, they are made with the `git` CLI, in a cleared environment
//! with no system or global config and fixed authors and dates.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use whetstone_core::CommitId;

use super::Repository;

/// The time of the first commit, 2026-01-01 00:00 UTC, in seconds since the
/// Unix epoch. Each later commit is a minute after the one before.
const FIRST_COMMIT_AT: u64 = 1_767_225_600;

/// A temporary directory holding a Git repository, deleted on drop.
#[derive(Debug)]
pub(crate) struct TestRepo {
    dir: TempDir,
    /// How many commits have been made, which dates the next one.
    commits: Cell<u64>,
}

impl TestRepo {
    /// A new repository with no commits.
    pub(crate) fn new() -> Self {
        let repo = Self {
            dir: TempDir::new().expect("create a temporary directory"),
            commits: Cell::new(0),
        };
        fs::create_dir(repo.root()).expect("create the repository's directory");
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo
    }

    /// The repository's working tree.
    pub(crate) fn root(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    /// Writes `contents` to `path` in the working tree, creating its
    /// directories.
    pub(crate) fn write(&self, path: &str, contents: impl AsRef<[u8]>) -> &Self {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().expect("a file has a parent directory"))
            .expect("create the file's directories");
        fs::write(path, contents).expect("write a file in the working tree");
        self
    }

    /// Makes `path` in the working tree a symbolic link to `target`.
    pub(crate) fn symlink(&self, path: &str, target: &str) -> &Self {
        std::os::unix::fs::symlink(target, self.root().join(path)).expect("make a symbolic link");
        self
    }

    /// Records a submodule at `path` pointing at the commit `id`, without
    /// cloning anything: its directory is left empty, as Git leaves a
    /// submodule that is not checked out.
    pub(crate) fn submodule(&self, path: &str, id: &str) -> &Self {
        fs::create_dir_all(self.root().join(path)).expect("create the submodule's directory");
        self.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{id},{path}"),
        ]);
        self
    }

    /// Makes `path` in the working tree executable.
    pub(crate) fn executable(&self, path: &str) -> &Self {
        use std::os::unix::fs::PermissionsExt as _;

        fs::set_permissions(self.root().join(path), fs::Permissions::from_mode(0o755))
            .expect("make a file executable");
        self
    }

    /// Commits everything in the working tree with `message`.
    pub(crate) fn commit(&self, message: &str) -> CommitId {
        self.git(&["add", "--all"]);
        let made = self.commits.get();
        self.commits.set(made + 1);
        let at = format!("@{} +0000", FIRST_COMMIT_AT + 60 * made);
        self.git_with(
            &["commit", "--quiet", "--allow-empty", "--message", message],
            &[("GIT_AUTHOR_DATE", &at), ("GIT_COMMITTER_DATE", &at)],
        );
        let id = self.git(&["rev-parse", "HEAD"]);
        CommitId::new(id.trim(), 7).expect("git prints a commit ID")
    }

    /// The repository, opened as Whetstone opens it.
    pub(crate) fn open(&self) -> Repository {
        Repository::open(&self.root()).expect("open the test repository")
    }

    /// Runs `git` with `args` in the repository and returns its output.
    fn git(&self, args: &[&str]) -> String {
        self.git_with(args, &[])
    }

    fn git_with(&self, args: &[&str], env: &[(&str, &str)]) -> String {
        let home = self.dir.path().join(".home");
        fs::create_dir_all(&home).expect("create git's home directory");
        let output = Command::new("git")
            .current_dir(self.root())
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("PATH is set"))
            .env("HOME", &home)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Ada Learner")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada Learner")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .envs(env.iter().copied())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git prints UTF-8")
    }
}

/// Every file under `dir`, relative to it, with `/` between directories,
/// in path order.
pub(crate) fn files_under(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        if fs::symlink_metadata(&path)
            .expect("read a path's metadata")
            .is_dir()
        {
            pending.extend(
                fs::read_dir(&path)
                    .expect("read a directory")
                    .map(|entry| entry.expect("a directory entry").path()),
            );
        } else {
            let relative = path.strip_prefix(dir).expect("under the directory");
            files.push(
                relative
                    .components()
                    .map(|part| part.as_os_str().to_str().expect("a UTF-8 path"))
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    files.sort();
    files
}
