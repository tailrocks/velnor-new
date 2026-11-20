// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use crate::run::scratch;
use crate::run_nextest::listed;

const FAKE_NEXTEST: &str = r#"#!/bin/sh
if [ "$2" = list ]; then
    dest=
    overwrite=
    while [ $# -gt 0 ]; do
        case "$1" in
            --extract-to) dest="$2" ;;
            --extract-overwrite) overwrite=1 ;;
        esac
        shift
    done
    if [ -n "$overwrite" ] && [ -n "$dest" ]; then
        chmod 644 "$dest/test-bin"
        sleep 0.5
        chmod 755 "$dest/test-bin"
    fi
    printf '%s\n' "$LIST_JSON"
    exit 0
fi
echo 'Starting 1 test across 1 binary' >&2
for _ in $(seq 1 40); do
    "$TREE/test-bin" || echo failed >> "$RUNNER_TEMP/exec-failures"
    sleep 0.02
done
echo 'Summary [   0.800s] 1 test run: 1 passed, 0 skipped' >&2
[ ! -e "$RUNNER_TEMP/exec-failures" ]
"#;

fn install(temp: &Path, tree: &Path) {
    fs::create_dir_all(tree).unwrap_or_else(|e| unreachable!("{e}"));
    let binary = tree.join("test-bin");
    fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let program = temp.join("cargo-nextest");
    fs::write(&program, FAKE_NEXTEST).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn the_early_list_of_an_archive_run_leaves_the_running_test_binaries_alone() {
    let temp = scratch("tests-run-archive-early-list");
    let tree = temp.join("tree");
    install(&temp, &tree);
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let extract = tree.display().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "cargo-nextest", "nextest", "run"])
        .args(["--archive-file", "a.tar.zst", "--extract-to", &extract])
        .args(["--extract-overwrite", "--workspace-remap", &extract])
        .args(["--no-fail-fast", "--no-tests=pass"])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", temp.join("github-output"))
        .env("LIST_JSON", listed(&[("kept", &["alpha"])]))
        .env("TREE", &tree)
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));

    let failures = fs::read_to_string(temp.join("exec-failures")).unwrap_or_default();
    assert_eq!(failures, "", "{output:?}");
    assert!(output.status.success(), "{output:?}");
}
