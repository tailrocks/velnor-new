//! Evasion fixture: Unix command extension must be flagged.

use std::os::unix::process::CommandExt;

fn run_evil() {
    let mut command = std::process::Command::new("evil");
    command.uid(0);
    let _ = command;
}
