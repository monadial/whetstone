//! The `whetstone` executable. All behaviour lives in the library crate.

use std::process::ExitCode;

fn main() -> ExitCode {
    whetstone::run(std::env::args_os(), &whetstone::Environment::from_process())
}
