// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use super::{audit, velnor_ci};
use crate::io::Repo;
use velnor_test_support::{ci_safe_cargo_env, real_cargo_str};

const DEBUG: &str = "rust/target/debug";

fn build(repo: &Repo) -> Output {
    let mut command = velnor_ci();
    ci_safe_cargo_env(&mut command)
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", &real_cargo_str()])
        .args(["build", "--workspace", "--all-targets"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn remove_script_compile(directory: &Path) -> usize {
    let mut removed = 0;
    for entry in fs::read_dir(directory).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        if path.is_dir() {
            removed += remove_script_compile(&path);
        } else if path.extension().is_some_and(|extension| extension == "d")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("build_script_build"))
        {
            let unit = path
                .parent()
                .and_then(Path::parent)
                .unwrap_or_else(|| unreachable!("{path:?}"));
            fs::remove_dir_all(unit).unwrap_or_else(|e| unreachable!("{e}"));
            return removed + 1;
        }
    }
    removed
}

fn replayed_script_repo() -> Repo {
    let repo = Repo::new("script-dep-info");
    let mise = crate::toolchain::mise_rust_pin();
    repo.write_all(&[
        ("mise.toml", mise.as_str()),
        (
            "rust/Cargo.toml",
            "[workspace]\nmembers = [\"lib\"]\nresolver = \"3\"\n",
        ),
        ("rust/Cargo.lock", "version = 4\n"),
        (
            "rust/lib/Cargo.toml",
            "[package]\nname = \"lib\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
        ),
        ("rust/lib/src/lib.rs", "pub fn value() -> u32 { 1 }\n"),
        (
            "rust/lib/build.rs",
            "fn main() { println!(\"cargo:rerun-if-changed=asset.txt\"); }\n",
        ),
        ("rust/lib/asset.txt", "first\n"),
    ]);
    repo
}

#[test]
fn a_build_script_run_whose_compile_left_no_dep_info_is_not_called_outputless() {
    let repo = replayed_script_repo();
    let built = build(&repo);
    assert!(built.status.success(), "{built:?}");
    assert_eq!(remove_script_compile(&repo.root().join(DEBUG)), 1);
    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("build script run without the dep-info of its compile"),
        "{stderr}"
    );
    assert!(!stderr.contains("no output"), "{stderr}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
