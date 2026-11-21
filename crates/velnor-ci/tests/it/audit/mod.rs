// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod boundary;
mod bounds;
mod build_clock;
mod build_directory;
mod build_script_source;
mod descriptors;
mod freshness;
mod gated;
mod global_options;
mod groups;
mod guards;
mod identity;
mod inventory;
mod link_targets;
mod links;
mod lock_identity;
mod managed_target;
mod migration;
mod nested_roots;
mod new_layout;
mod optional;
mod partial;
mod path_dependencies;
mod per_root;
mod reads;
mod selection;
mod separate_build;
mod stale_run;
mod stand_ins;

use std::ffi::OsStr;
use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;

pub(super) use path_dependencies::{
    path_dependency_units, path_dependency_units_in, root_package_with_path_dependencies,
};

pub(super) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .env_remove("CARGO_TARGET_DIR")
        .env("MBX_BUILD_SCRIPT_EXECUTION", "0")
        .env("MBX_SHARE_OUT_DIR", "0")
        .env_remove("VELNOR_AUDIT_MAX_ENTRIES");
    command
}

pub(super) fn velnor_ci() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("VELNOR_AUDIT_MAX_ENTRIES");
    command
}

pub(super) fn audit(repo: &Repo, args: &[&str]) -> Output {
    velnor_ci()
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

pub(super) fn placed(repo: &Repo, target: &str) {
    if let Some(parent) = std::path::Path::new(target)
        .parent()
        .filter(|parent| parent.file_name().is_some_and(|name| name != "target"))
    {
        unit(repo, &format!("{}/CACHEDIR.TAG", parent.display()), "");
    }
}

pub(super) fn workspace_below_the_root() -> Repo {
    let repo = Repo::new("audit");
    unit(&repo, ".gitignore", "target/\n");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    unit(
        &repo,
        "rust/Cargo.toml",
        "[workspace]\nmembers = [\"app\", \"lib\", \"macros\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "rust/Cargo.lock", "version = 4\n");
    for (package, extra) in [
        (
            "app",
            "\n[features]\nextra = []\n\n[[bin]]\nname = \"extra\"\npath = \"src/extra.rs\"\nrequired-features = [\"extra\"]\n",
        ),
        ("lib", "build = \"build.rs\"\n"),
    ] {
        unit(
            &repo,
            &format!("rust/{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nedition = \"2024\"\n{extra}"),
        );
        unit(&repo, &format!("rust/{package}/src/lib.rs"), "");
        unit(&repo, &format!("rust/{package}/tests/it.rs"), "");
    }
    unit(
        &repo,
        "rust/macros/Cargo.toml",
        "[package]\nname = \"macros\"\nedition = \"2024\"\n\n[lib]\nproc-macro = true\n",
    );
    unit(&repo, "rust/macros/src/lib.rs", "");
    unit(&repo, "rust/lib/build.rs", "fn main() {}\n");
    unit(&repo, "rust/app/build.rs", "fn main() {}\n");
    unit(&repo, "rust/app/src/extra.rs", "fn main() {}\n");
    unit(&repo, "rust/app/data.txt", "");
    for file in [
        "greeting.txt",
        "app-test.txt",
        "lib-test.txt",
        "build.txt",
        "app-build.txt",
        "old.txt",
    ] {
        unit(&repo, &format!("shared/{file}"), "");
    }
    repo.git(&["add", "--all"]);
    repo
}

pub(super) fn own_units(repo: &Repo, target: &str, left_out: &[&str]) {
    own_units_in(repo, target, target, left_out);
}

const OWN_UNITS: &[(&str, bool, &str, &str, &str)] = &[
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
    placed(repo, target);
    for &(key, on_host, file, text, fingerprint) in OWN_UNITS {
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
