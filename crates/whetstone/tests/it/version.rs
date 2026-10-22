//! What `whetstone --version` should print, worked out from git here, apart
//! from the build script, so the tests check the build script rather than
//! repeat it.

use std::process::Command;

/// What `whetstone --version` prints after `whetstone `: the version, then
/// the first seven hex digits of the commit this checkout is at and that
/// commit's date. Where git can't say, as the build script then couldn't,
/// it is the version alone.
pub(crate) fn built_version() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let head = git(&["rev-parse", "HEAD"]);
    let date = git(&["show", "--no-patch", "--format=%cs", "HEAD"]);
    match (head.as_deref().and_then(|head| head.get(..7)), date) {
        (Some(short), Some(date)) => format!("{version} ({short} {date})"),
        _ => version.to_owned(),
    }
}

/// What `git` prints with `args` in this crate's directory, trimmed, or
/// `None` when it fails.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}
