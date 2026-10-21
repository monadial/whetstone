//! The version `whetstone --version` prints: the package version, then the
//! commit the binary was built from and that commit's date, so a bug report
//! names its exact source (ADR 0003). `build.rs` asks git for the commit,
//! and the integration tests include this file to cover it.

/// The `git log` format whose output [`long_version`] reads: the full
/// commit hash, a space and the commit date as `YYYY-MM-DD`.
pub(crate) const GIT_LOG_FORMAT: &str = "--format=%H %cs";

/// `version`, followed by the short hash and date of the commit that
/// `git_log` describes in [`GIT_LOG_FORMAT`]: `0.1.0 (04d5bb5 2026-10-21)`.
/// The date is the commit's own, not the build's, so building one commit
/// twice gives one version. Without git output, or with output that isn't a
/// hash and a date, it is `version` alone.
pub(crate) fn long_version(version: &str, git_log: Option<&str>) -> String {
    match git_log.and_then(commit) {
        Some((hash, date)) => format!("{version} ({hash} {date})"),
        None => version.to_owned(),
    }
}

/// The short hash and the date in one line of [`GIT_LOG_FORMAT`] output.
fn commit(git_log: &str) -> Option<(&str, &str)> {
    let (hash, date) = git_log.trim_end().split_once(' ')?;
    // SHA-1 or SHA-256, as git prints it.
    let is_hash = matches!(hash.len(), 40 | 64)
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    let is_date = date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    if is_hash && is_date {
        Some((hash.get(..7)?, date))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::long_version;

    const HASH: &str = "04d5bb5f90f4f8191feb4539a9a268aeb03add77";

    #[test]
    fn names_the_short_hash_and_the_commit_date() {
        let log = format!("{HASH} 2026-10-21\n");

        assert_eq!(
            long_version("0.1.0", Some(&log)),
            "0.1.0 (04d5bb5 2026-10-21)"
        );
    }

    #[test]
    fn reads_a_sha_256_hash() {
        let log = format!("{HASH}{} 2026-10-21", "0".repeat(24));

        assert_eq!(
            long_version("0.1.0-rc.1", Some(&log)),
            "0.1.0-rc.1 (04d5bb5 2026-10-21)"
        );
    }

    #[test]
    fn is_the_version_alone_without_git() {
        assert_eq!(long_version("0.1.0", None), "0.1.0");
    }

    #[test]
    fn is_the_version_alone_when_git_says_something_else() {
        for log in [
            "",
            "fatal: not a git repository",
            "04d5bb5 2026-10-21",
            &format!("{} 2026-10-21", HASH.to_uppercase()),
            &format!("{HASH} 21 October 2026"),
            &format!("{HASH} 2026/10/21"),
            HASH,
        ] {
            assert_eq!(long_version("0.1.0", Some(log)), "0.1.0", "{log:?}");
        }
    }
}
