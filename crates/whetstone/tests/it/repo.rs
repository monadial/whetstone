//! Throwaway Git repositories with real commits, for tests that start
//! Whetstone inside a learner's repository.
//!
//! They are built with the `git` CLI, which makes every shape a test needs
//! (merges, root commits, shallow clones, submodules, symlinks, renames,
//! linked worktrees, bare repositories) the way learners' repositories are
//! made, without a Git library with write features in the build. Each `git`
//! runs with a cleared environment, no system or global config, and fixed
//! authors, committers and dates, so commit IDs are the same on every
//! machine and snapshots can show them.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use tempfile::TempDir;

/// Who makes every commit, as author and committer.
const AUTHOR: (&str, &str) = ("Ada Learner", "ada@example.com");
/// The time of the first commit, 2026-01-01 00:00 UTC, in seconds since the
/// Unix epoch. Each later commit is a minute after the one before.
const FIRST_COMMIT_AT: u64 = 1_767_225_600;

/// A temporary directory holding a Git repository, and room beside it for
/// clones and linked worktrees. Everything is deleted when it is dropped.
#[derive(Debug)]
pub(crate) struct TestRepo {
    dir: TempDir,
    root: PathBuf,
    /// How many commits `git` has made here, which dates the next one.
    commits: Cell<u64>,
}

impl TestRepo {
    /// A new repository with no commits, on branch `main`, in a directory
    /// called `name`: the repository name home shows.
    pub(crate) fn new(name: &str) -> Self {
        Self::init(name, &[])
    }

    /// A new bare repository with no commits, in a directory called `name`.
    pub(crate) fn bare(name: &str) -> Self {
        Self::init(name, &["--bare"])
    }

    fn init(name: &str, flags: &[&str]) -> Self {
        let dir = TempDir::new().expect("create a temporary directory");
        let root = dir.path().join(name);
        let repo = Self {
            dir,
            root,
            commits: Cell::new(0),
        };
        let root = repo.root.to_str().expect("a UTF-8 path");
        let mut args = vec!["init", "--quiet", "--initial-branch=main"];
        args.extend(flags);
        args.push(root);
        repo.git_in(repo.dir.path(), &args);
        repo
    }

    /// The repository's root: its working tree, or the bare repository.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// `path` inside the repository's root.
    pub(crate) fn path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// A place beside the repository, outside it, called `name`.
    pub(crate) fn beside(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Writes `contents` to `path` in the working tree, creating its
    /// directories.
    pub(crate) fn write(&self, path: &str, contents: impl AsRef<[u8]>) -> &Self {
        let path = self.path(path);
        fs::create_dir_all(path.parent().expect("a file has a parent directory"))
            .expect("create the file's directories");
        fs::write(path, contents).expect("write a file in the working tree");
        self
    }

    /// Makes `path` in the working tree a symbolic link to `target`.
    pub(crate) fn symlink(&self, path: &str, target: &str) -> &Self {
        std::os::unix::fs::symlink(target, self.path(path)).expect("make a symbolic link");
        self
    }

    /// Adds a submodule at `path`: a repository beside this one, named
    /// after the last part of `path`, with one commit. Its URL is relative,
    /// so commit IDs stay the same wherever the test runs.
    pub(crate) fn add_submodule(&self, path: &str) -> &Self {
        let name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a submodule path ends in a UTF-8 name");
        let submodule = self.beside(name);
        self.git_in(
            self.dir.path(),
            &[
                "init",
                "--quiet",
                "--initial-branch=main",
                submodule.to_str().expect("a UTF-8 path"),
            ],
        );
        fs::write(submodule.join("README.md"), format!("# {name}\n"))
            .expect("write the submodule's readme");
        self.git_in(&submodule, &["add", "--all"]);
        self.dated_in(&submodule, &["commit", "--quiet", "--message", "Start"]);
        self.git(&[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            &format!("../{name}"),
            path,
        ]);
        self
    }

    /// Commits everything in the working tree with `message`, and returns
    /// the commit's full ID.
    pub(crate) fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.dated(&["commit", "--quiet", "--allow-empty", "--message", message]);
        self.head()
    }

    /// Merges `branch` into the current branch with a merge commit, even
    /// where it could fast-forward, and returns the merge's full ID.
    pub(crate) fn merge(&self, branch: &str, message: &str) -> String {
        self.dated(&["merge", "--quiet", "--no-ff", "--message", message, branch]);
        self.head()
    }

    /// A shallow clone of the repository beside it, called `name`, with
    /// only the newest `depth` commits, so the oldest of them has its
    /// parent missing. Returns its root.
    pub(crate) fn shallow_clone(&self, name: &str, depth: usize) -> PathBuf {
        let clone = self.beside(name);
        // Git clones shallow only over a transport, `file://` here.
        let url = format!("file://{}", self.root.display());
        self.git_in(
            self.dir.path(),
            &[
                "clone",
                "--quiet",
                "--depth",
                &depth.to_string(),
                &url,
                clone.to_str().expect("a UTF-8 path"),
            ],
        );
        clone
    }

    /// Removes the loose object `id` from the repository, as a partial
    /// clone lacks the files it has not fetched.
    pub(crate) fn remove_object(&self, id: &str) {
        let (directory, file) = id.split_at(2);
        fs::remove_file(self.path(&format!(".git/objects/{directory}/{file}")))
            .expect("remove a loose object");
    }

    /// The full ID of the commit HEAD points to.
    pub(crate) fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Runs `git` with `args`, which make a commit, dated a minute after
    /// the commit before.
    fn dated(&self, args: &[&str]) {
        self.dated_in(&self.root, args);
    }

    /// Runs `git` with `args` in `dir`, which make a commit, dated a minute
    /// after the commit before.
    fn dated_in(&self, dir: &Path, args: &[&str]) {
        let made = self.commits.get();
        self.commits.set(made + 1);
        let at = format!("@{} +0000", FIRST_COMMIT_AT + 60 * made);
        self.git_with(
            dir,
            args,
            &[("GIT_AUTHOR_DATE", &at), ("GIT_COMMITTER_DATE", &at)],
        );
    }

    /// Runs `git` with `args` in the repository's root and returns its
    /// standard output.
    pub(crate) fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.root, args)
    }

    /// Runs `git` with `args` in `dir` and returns its standard output.
    pub(crate) fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        self.git_with(dir, args, &[])
    }

    fn git_with(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
        let home = self.dir.path().join(".home");
        fs::create_dir_all(&home).expect("create git's home directory");
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("PATH is set"))
            .env("HOME", &home)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", AUTHOR.0)
            .env("GIT_AUTHOR_EMAIL", AUTHOR.1)
            .env("GIT_COMMITTER_NAME", AUTHOR.0)
            .env("GIT_COMMITTER_EMAIL", AUTHOR.1)
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

    /// Everything on disk in the temporary directory: the repository, its
    /// `.git` and anything beside it.
    pub(crate) fn on_disk(&self) -> OnDisk {
        OnDisk::of(self.dir.path())
    }

    /// Asserts that nothing on disk changed since `before`, which
    /// [`TestRepo::on_disk`] took: no file or directory created, removed or
    /// written, no lock file left behind and no `.whetstone/` created.
    pub(crate) fn assert_untouched(&self, before: &OnDisk) {
        let after = self.on_disk();
        assert_eq!(after.lock_files(), Vec::<&Path>::new(), "no lock files");
        let whetstone = self
            .root
            .strip_prefix(self.dir.path())
            .expect("the root is in the temporary directory")
            .join(".whetstone");
        assert!(
            before.has(&whetstone) || !after.has(&whetstone),
            "no .whetstone/ created"
        );
        assert!(
            after == *before,
            "nothing on disk changed:\n{:#?}",
            before.difference(&after)
        );
    }
}

/// What is on disk under a directory: every path, with what it is, its size
/// and when it was last modified. Two are equal only if nothing was
/// created, removed or written in between, since writing a file changes its
/// time and creating or removing one, even briefly, changes its
/// directory's.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OnDisk(BTreeMap<PathBuf, Entry>);

/// One path on disk.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    kind: &'static str,
    len: u64,
    modified: SystemTime,
}

impl OnDisk {
    fn of(dir: &Path) -> Self {
        let mut entries = BTreeMap::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(path) = pending.pop() {
            let metadata = fs::symlink_metadata(&path).expect("read a path's metadata");
            let kind = if metadata.is_dir() {
                pending.extend(
                    fs::read_dir(&path)
                        .expect("read a directory")
                        .map(|entry| entry.expect("a directory entry").path()),
                );
                "directory"
            } else if metadata.is_symlink() {
                "symlink"
            } else {
                "file"
            };
            let relative = path.strip_prefix(dir).expect("under the directory");
            entries.insert(
                relative.to_path_buf(),
                Entry {
                    kind,
                    len: metadata.len(),
                    modified: metadata.modified().expect("a modification time"),
                },
            );
        }
        Self(entries)
    }

    /// The paths that end in `.lock`, as Git's lock files do.
    fn lock_files(&self) -> Vec<&Path> {
        self.0
            .keys()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "lock")
            })
            .map(PathBuf::as_path)
            .collect()
    }

    /// Whether `path`, relative to the temporary directory, is on disk.
    fn has(&self, path: &Path) -> bool {
        self.0.contains_key(path)
    }

    /// The paths that differ between this and `other`: created, removed or
    /// changed.
    fn difference<'a>(&'a self, other: &'a Self) -> Vec<&'a Path> {
        self.0
            .keys()
            .chain(other.0.keys())
            .filter(|path| self.0.get(*path) != other.0.get(*path))
            .map(PathBuf::as_path)
            .collect()
    }
}
