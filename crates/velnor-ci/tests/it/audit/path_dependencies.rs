// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::{placed, unit};
use crate::io::Repo;

pub(crate) fn root_package_with_path_dependencies() -> Repo {
    let repo = Repo::new("audit-owners");
    unit(&repo, ".gitignore", "target/\n");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    unit(&repo, ".velnor/rust-ci.toml", "exclude = [\"vendor\"]\n");
    unit(
        &repo,
        "Cargo.toml",
        "[package]\nname = \"top\"\nedition = \"2024\"\n\n[[test]]\nname = \"qa\"\npath = \"qa.rs\"\n\n[[test]]\nname = \"ext\"\npath = \"ext.rs\"\n\n[dependencies]\nside = { path = \"vendor/side\" }\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    unit(
        &repo,
        "vendor/side/Cargo.toml",
        "[package]\nname = \"side\"\nedition = \"2024\"\n",
    );
    unit(
        &repo,
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nedition = \"2024\"\n\n[[test]]\nname = \"ext\"\npath = \"../ext.rs\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\" }\n",
    );
    unit(&repo, "Cargo.lock", "version = 4\n");
    unit(
        &repo,
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    unit(
        &repo,
        "vendor/leaf/Cargo.toml",
        "[package]\nname = \"leaf\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    );
    for file in [
        "src/lib.rs",
        "other/src/lib.rs",
        "qa.rs",
        "data/qa.txt",
        "README.md",
        "NOTICE",
        "vendor/mid/src/lib.rs",
        "vendor/mid/build.rs",
        "vendor/leaf/src/lib.rs",
        "vendor/leaf/build.rs",
        "vendor/side/src/lib.rs",
        "LICENSE",
        "ext.rs",
        "data/ext.txt",
    ] {
        unit(&repo, file, "");
    }
    fs::create_dir_all(repo.root().join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("../README.md", repo.root().join("shared/link.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    repo
}

pub(crate) fn path_dependency_units(repo: &Repo) {
    path_dependency_units_in(repo, "target/debug");
}

pub(crate) fn path_dependency_units_in(repo: &Repo, target: &str) {
    placed(repo, target);
    for (file, text, fingerprint) in [
        (
            "deps/top-0000000000000001.d",
            "src/lib.rs shared/link.txt",
            "top-0000000000000001/lib-top",
        ),
        (
            "deps/other-0000000000000005.d",
            "other/src/lib.rs",
            "other-0000000000000005/lib-other",
        ),
        (
            "deps/other-000000000000000c.d",
            "other/src/lib.rs",
            "other-000000000000000c/test-lib-other",
        ),
        (
            "deps/top-000000000000000d.d",
            "src/lib.rs",
            "top-000000000000000d/test-lib-top",
        ),
        (
            "deps/qa-0000000000000002.d",
            "qa.rs data/qa.txt",
            "top-0000000000000002/test-integration-test-qa",
        ),
        (
            "deps/mid-0000000000000003.d",
            "vendor/mid/src/lib.rs vendor/mid/src/../../../NOTICE",
            "mid-0000000000000003/lib-mid",
        ),
        (
            "build/mid-0000000000000006/build_script_build-0000000000000006.d",
            "vendor/mid/build.rs",
            "mid-0000000000000006/build-script-build-script-build",
        ),
        (
            "deps/leaf-0000000000000007.d",
            "vendor/leaf/src/lib.rs",
            "leaf-0000000000000007/lib-leaf",
        ),
        (
            "build/leaf-0000000000000008/build_script_build-0000000000000008.d",
            "vendor/leaf/build.rs",
            "leaf-0000000000000008/build-script-build-script-build",
        ),
        (
            "deps/ext-000000000000000a.d",
            "ext.rs data/ext.txt",
            "top-000000000000000a/test-integration-test-ext",
        ),
        (
            "deps/ext-000000000000000b.d",
            "ext.rs data/ext.txt",
            "other-000000000000000b/test-integration-test-ext",
        ),
    ] {
        unit(
            repo,
            &format!("{target}/{file}"),
            &format!("out.d: {text}\n"),
        );
        if !fingerprint.is_empty() {
            unit(repo, &format!("{target}/.fingerprint/{fingerprint}"), "");
        }
    }
    unit(
        repo,
        &format!("{target}/build/mid-0000000000000004/output"),
        "cargo:rerun-if-changed=build.rs\ncargo:rerun-if-changed=../../schema/future.json\ncargo:rerun-if-changed=../../target/debug/gen.txt\n",
    );
    unit(
        repo,
        &format!("{target}/build/leaf-0000000000000009/output"),
        "cargo:rerun-if-changed=build.rs\ncargo:rerun-if-changed=../../LICENSE\n",
    );
    prepare_path_dependency_outputs(repo, target);
}

fn prepare_path_dependency_outputs(repo: &Repo, target: &str) {
    unit(
        repo,
        &format!("{target}/deps/side-000000000000000e.d"),
        "out.d: vendor/side/src/lib.rs\n",
    );
    unit(
        repo,
        &format!("{target}/.fingerprint/side-000000000000000e/lib-side"),
        "",
    );
    let target_directory = repo.root().join("target");
    for output in [
        format!("{target}/build/mid-0000000000000004/output"),
        format!("{target}/build/leaf-0000000000000009/output"),
    ] {
        super::inventory::build(repo, &output, Some(&target_directory));
    }
}
