//! Whetstone's directories come from the injected environment, following the
//! XDG Base Directory layout.

use std::path::PathBuf;

use whetstone::{Dirs, Environment};

fn dirs(vars: &[(&str, &str)]) -> Result<Dirs, String> {
    let environment: Environment = vars.iter().copied().collect();
    Dirs::resolve(&environment).map_err(|error| error.to_string())
}

#[test]
fn xdg_variables_choose_the_directories() {
    assert_eq!(
        dirs(&[
            ("HOME", "/home/learner"),
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_STATE_HOME", "/xdg/state"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
        ]),
        Ok(Dirs {
            config: PathBuf::from("/xdg/config/whetstone"),
            state: PathBuf::from("/xdg/state/whetstone"),
            cache: PathBuf::from("/xdg/cache/whetstone"),
        })
    );
}

#[test]
fn without_xdg_variables_the_directories_are_under_home() {
    assert_eq!(
        dirs(&[("HOME", "/home/learner")]),
        Ok(Dirs {
            config: PathBuf::from("/home/learner/.config/whetstone"),
            state: PathBuf::from("/home/learner/.local/state/whetstone"),
            cache: PathBuf::from("/home/learner/.cache/whetstone"),
        })
    );
}

#[test]
fn empty_or_relative_xdg_variables_are_ignored() {
    assert_eq!(
        dirs(&[
            ("HOME", "/home/learner"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_STATE_HOME", "relative/state"),
            ("XDG_CACHE_HOME", "relative/cache"),
        ]),
        Ok(Dirs {
            config: PathBuf::from("/home/learner/.config/whetstone"),
            state: PathBuf::from("/home/learner/.local/state/whetstone"),
            cache: PathBuf::from("/home/learner/.cache/whetstone"),
        })
    );
}

#[test]
fn without_home_or_xdg_variables_there_is_no_directory() {
    assert_eq!(
        dirs(&[("XDG_STATE_HOME", "/xdg/state")]),
        Err(
            "cannot find the config directory: set HOME or XDG_CONFIG_HOME to an absolute path"
                .to_owned()
        )
    );
}
