use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::*;
use crate::tool_seed_test_support::mock_trust_commands;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "velnor-seed-admission-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path.canonicalize().expect("canonical scratch")
}

fn seed(root: &Path) {
    fs::create_dir_all(root.join("mise/tree")).expect("mise tree");
    fs::write(root.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("provenance");
}

fn run(script: &str, root: &Path, mounts: &str) -> Output {
    run_with_file_size(script, root, mounts, "64")
}

fn run_with_file_size(script: &str, root: &Path, mounts: &str, file_size: &str) -> Output {
    let command_script = format!("set -euo pipefail\n{script}");
    Command::new("bash")
        .arg("-c")
        .arg(command_script)
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_ROOT", root)
        .env("SEED_TEST_MOUNTS", mounts)
        .env("SEED_TEST_FILE_SIZE", file_size)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .output()
        .expect("run seed guard")
}

fn trust_script(test_dir: &Path) -> String {
    let guard = trusted_seed_guard(SEED_ROOT).expect("guard");
    let script = format!("{guard}\ntrusted_seed_is_trusted \"$SEED_TEST_ROOT\"");
    mock_trust_commands(&script, test_dir)
}

mod mounts;
mod trees;
