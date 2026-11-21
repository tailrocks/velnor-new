// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::io::Repo;

const SAMPLE: &str = "../../generator/samples/check-configurations/view";
const CONFIG: &str =
    "[[check_configurations]]\nroot = \".\"\nconfigurations = \"ci/configurations.json\"\n";
const SUBMODULE: &str = "1111111111111111111111111111111111111111";

fn copy_tree(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap_or_else(|e| unreachable!("{e}")) {
        let entry = entry.unwrap_or_else(|e| unreachable!("{e}"));
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&target).unwrap_or_else(|e| unreachable!("{e}"));
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
}

#[test]
fn a_configurations_path_that_became_a_submodule_is_refused_by_scope_detection() {
    let repo = Repo::new("configurations-gitlink");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLE),
        repo.root(),
    );
    repo.write_unstaged(".velnor/rust-ci.toml", CONFIG);
    let digest = "a".repeat(64);
    let lock = fs::read_to_string(repo.root().join(".velnor/rust-ci.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .replace("0.1.111", "0.1.112")
        .replace(
            "[velnor-gen-binary]\n",
            &format!("[velnor-gen-binary]\nmacos-arm64 = \"{digest}\"\n"),
        );
    repo.write_unstaged(".velnor/rust-ci.lock", &lock);
    repo.write_unstaged("ci/configurations.json", "[]\n");
    for lib in ["src/lib.rs", "tools/a/src/lib.rs", "tools/b/src/lib.rs"] {
        repo.write_unstaged(lib, "pub fn f() {}\n");
    }
    repo.git(&["add", "--all"]);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    repo.git(&["rm", "--cached", "--quiet", "ci/configurations.json"]);
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{SUBMODULE},ci/configurations.json"),
    ]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", "save", "--target-dir", "target"])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("a git submodule"), "{stderr}");
}
