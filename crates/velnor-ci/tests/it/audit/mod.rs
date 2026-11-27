// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod boundary;
mod bounds;
mod build_clock;
mod build_directory;
mod build_script_source;
mod failure_identity;
mod freshness;
mod gated;
mod generated_commands;
mod global_options;
mod groups;
mod guards;
mod identity;
mod inventory;
mod invocation_roots;
mod link_targets;
mod links;
mod lock_identity;
mod managed_target;
mod migration;
mod nested_roots;
mod new_layout;
mod optional;
mod outside_reads;
mod own_units;
mod partial;
mod path_dependency_fixture;
mod per_root;
mod plain_build;
mod reads;
mod real_cargo;
mod real_includes;
mod relative_build_dir;
mod script_dep_info;
mod selection;
mod separate_build;
mod shared_build;
mod stale_run;
mod stand_ins;
mod toolchain_probe;
mod tracked_build;

use std::ffi::OsStr;
use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;

use own_units::{OWN_UNITS, own_units, own_units_in};
pub(super) use path_dependency_fixture::{path_dependency_units, path_dependency_units_in};

pub(super) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = crate::run::command_without_nextest_environment(program);
    command
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env("MBX_BUILD_SCRIPT_EXECUTION", "0")
        .env("MBX_SHARE_OUT_DIR", "0")
        .env_remove("VELNOR_AUDIT_MAX_ENTRIES");
    command
}

pub(super) fn velnor_ci() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    velnor_test_support::keep_mise_cargo_home(&mut command);
    command
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
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

pub(super) fn root_package_with_path_dependencies() -> Repo {
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
