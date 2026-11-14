// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod guards;
mod identity;
mod reads;

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;

pub(super) fn audit(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["inputs", "audit"])
        .args(args)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn unit(repo: &Repo, path: &str, text: &str) {
    let path = repo.root().join(path);
    fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
}

pub(super) fn workspace_below_the_root() -> Repo {
    let repo = Repo::new("audit");
    repo.write(".gitignore", "target/\n");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "rust/Cargo.toml",
        "[workspace]\nmembers = [\"app\", \"lib\", \"macros\"]\nresolver = \"3\"\n",
    );
    repo.write("rust/Cargo.lock", "version = 4\n");
    for (package, extra) in [
        (
            "app",
            "\n[features]\nextra = []\n\n[[bin]]\nname = \"extra\"\npath = \"src/extra.rs\"\nrequired-features = [\"extra\"]\n",
        ),
        ("lib", "build = \"build.rs\"\n"),
    ] {
        repo.write(
            &format!("rust/{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nedition = \"2024\"\n{extra}"),
        );
        repo.write(&format!("rust/{package}/src/lib.rs"), "");
        repo.write(&format!("rust/{package}/tests/it.rs"), "");
    }
    repo.write(
        "rust/macros/Cargo.toml",
        "[package]\nname = \"macros\"\nedition = \"2024\"\n\n[lib]\nproc-macro = true\n",
    );
    repo.write("rust/macros/src/lib.rs", "");
    repo.write("rust/lib/build.rs", "fn main() {}\n");
    repo.write("rust/app/build.rs", "fn main() {}\n");
    repo.write("rust/app/src/extra.rs", "fn main() {}\n");
    repo.write("rust/app/data.txt", "");
    for file in [
        "greeting.txt",
        "app-test.txt",
        "lib-test.txt",
        "build.txt",
        "app-build.txt",
        "old.txt",
    ] {
        repo.write(&format!("shared/{file}"), "");
    }
    repo
}

pub(super) fn own_units(repo: &Repo, target: &str, left_out: &[&str]) {
    own_units_in(repo, target, target, left_out);
}

const OWN_UNITS: [(&str, bool, &str, &str, &str); 12] = [
    (
        "app",
        false,
        "deps/app-f000000000000001.d",
        "app/src/lib.rs",
        "app-f000000000000001/lib-app",
    ),
    (
        "app",
        false,
        "deps/app-f000000000000009.d",
        "app/src/lib.rs",
        "app-f000000000000009/test-lib-app",
    ),
    (
        "app-it",
        false,
        "deps/it-f000000000000002.d",
        "app/tests/it.rs",
        "app-f000000000000002/test-integration-test-it",
    ),
    (
        "app-build",
        true,
        "build/app-f000000000000003/build_script_build-f000000000000003.d",
        "app/build.rs",
        "",
    ),
    (
        "app-build",
        false,
        "build/app-f000000000000004/output",
        "",
        "",
    ),
    (
        "lib",
        false,
        "deps/lib-f000000000000005.d",
        "lib/src/lib.rs",
        "lib-f000000000000005/lib-lib",
    ),
    (
        "lib",
        false,
        "deps/lib-f00000000000000a.d",
        "lib/src/lib.rs",
        "lib-f00000000000000a/test-lib-lib",
    ),
    (
        "lib-it",
        false,
        "deps/it-f000000000000006.d",
        "lib/tests/it.rs",
        "lib-f000000000000006/test-integration-test-it",
    ),
    (
        "lib-build",
        true,
        "build/lib-f000000000000007/build_script_build-f000000000000007.d",
        "lib/build.rs",
        "",
    ),
    (
        "lib-build",
        false,
        "build/lib-f000000000000008/output",
        "",
        "",
    ),
    (
        "macros",
        true,
        "deps/macros-f00000000000000b.d",
        "macros/src/lib.rs",
        "macros-f00000000000000b/lib-macros",
    ),
    (
        "macros",
        true,
        "deps/macros-f00000000000000c.d",
        "macros/src/lib.rs",
        "macros-f00000000000000c/test-lib-macros",
    ),
];

pub(super) fn own_units_in(repo: &Repo, host: &str, target: &str, left_out: &[&str]) {
    for (key, on_host, file, text, fingerprint) in OWN_UNITS {
        if left_out.contains(&key) {
            continue;
        }
        let directory = if on_host { host } else { target };
        let text = if text.is_empty() {
            String::new()
        } else {
            format!("out.d: {text}\n")
        };
        unit(repo, &format!("{directory}/{file}"), &text);
        if !fingerprint.is_empty() {
            unit(repo, &format!("{directory}/.fingerprint/{fingerprint}"), "");
        }
    }
}

pub(super) fn root_package_with_path_dependencies() -> Repo {
    let repo = Repo::new("audit-owners");
    repo.write(".gitignore", "target/\n");
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(".velnor/rust-ci.toml", "exclude = [\"vendor\"]\n");
    repo.write(
        "Cargo.toml",
        "[package]\nname = \"top\"\nedition = \"2024\"\n\n[[test]]\nname = \"qa\"\npath = \"qa.rs\"\n\n[[test]]\nname = \"ext\"\npath = \"ext.rs\"\n\n[dependencies]\nside = { path = \"vendor/side\" }\n\n[workspace]\nmembers = [\"other\"]\nexclude = [\"vendor\"]\n",
    );
    repo.write(
        "vendor/side/Cargo.toml",
        "[package]\nname = \"side\"\nedition = \"2024\"\n",
    );
    repo.write(
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nedition = \"2024\"\n\n[[test]]\nname = \"ext\"\npath = \"../ext.rs\"\n\n[dependencies]\nmid = { path = \"../vendor/mid\" }\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    repo.write(
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    repo.write(
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
        repo.write(file, "");
    }
    fs::create_dir_all(repo.root().join("shared")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("../README.md", repo.root().join("shared/link.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    repo
}

pub(super) fn path_dependency_units(repo: &Repo) {
    path_dependency_units_in(repo, "target/debug");
}

pub(super) fn path_dependency_units_in(repo: &Repo, target: &str) {
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
}
