// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Output;

use super::{velnor_ci, workspace_below_the_root};

fn audit_from_rust(root: &Path, value: &str) -> Output {
    velnor_ci()
        .current_dir(root.join("rust"))
        .env("CARGO_BUILD_BUILD_DIR", value)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_relative_build_directory_is_audited_where_cargo_dash_c_writes_it() {
    let repo = workspace_below_the_root();
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", "bd")
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "rust",
            "build",
            "--workspace",
            "--all-targets",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    let written = repo.root().join("rust/bd");
    assert!(written.join("debug").is_dir(), "{}", written.display());
    assert!(!repo.root().join("bd").exists());
    let relative = audit_from_rust(repo.root(), "bd");
    assert!(relative.status.success(), "{relative:?}");
    let absolute = audit_from_rust(repo.root(), &written.display().to_string());
    assert!(absolute.status.success(), "{absolute:?}");
    assert_eq!(relative.stdout, absolute.stdout);
    assert!(
        !String::from_utf8_lossy(&relative.stdout).contains("from 0 unit files"),
        "{relative:?}"
    );
}
