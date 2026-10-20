//! The `whetstone-labels` executable. All behaviour lives in the library
//! crate.

use std::process::ExitCode;

fn main() -> ExitCode {
    whetstone_labels::run(
        std::env::args_os(),
        &whetstone_labels::Environment::from_process(),
    )
}
