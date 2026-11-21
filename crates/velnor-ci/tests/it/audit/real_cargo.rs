// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use super::{unit, velnor_ci};
use crate::io::Repo;

fn workspace() -> Repo {
    let repo = Repo::new("audit-real");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    unit(
        &repo,
        "Cargo.toml",
        "[workspace]\nmembers = [\"my-app\", \"my-lib\", \"my-macro\", \"my-tool\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "Cargo.lock", "version = 4\n");
    let package = |name: &str, extra: &str| {
        let manifest = format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n{extra}");
        unit(&repo, &format!("{name}/Cargo.toml"), &manifest);
    };
    package("my-lib", "");
    package("my-macro", "\n[lib]\nproc-macro = true\n");
    package(
        "my-app",
        "build = \"build.rs\"\n\n[dependencies]\nmy-lib = { path = \"../my-lib\" }\nmy-macro = { path = \"../my-macro\" }\n",
    );
    package(
        "my-tool",
        "\n[dependencies]\nmy-lib = { path = \"../my-lib\" }\n",
    );
    unit(&repo, "my-lib/src/lib.rs", "pub fn lib() -> u32 { 1 }\n");
    unit(
        &repo,
        "my-macro/src/lib.rs",
        "extern crate proc_macro;\n#[proc_macro]\npub fn one(_: proc_macro::TokenStream) -> proc_macro::TokenStream {\n    \"1\".parse().unwrap()\n}\n",
    );
    unit(&repo, "my-app/build.rs", "fn main() {}\n");
    unit(
        &repo,
        "my-app/src/lib.rs",
        "pub fn app() -> u32 { my_lib::lib() + my_macro::one!() }\n",
    );
    let main = "fn main() { println!(\"{}\", my_app::app()); }\n";
    unit(&repo, "my-app/src/main.rs", main);
    unit(&repo, "my-app/examples/demo-ex.rs", main);
    unit(
        &repo,
        "my-app/tests/it.rs",
        "#[test]\nfn app() {\n    assert_eq!(my_app::app(), 2);\n}\n",
    );
    let tool = "fn main() { println!(\"{}\", my_lib::lib()); }\n";
    unit(&repo, "my-tool/src/main.rs", tool);
    unit(&repo, "my-tool/examples/tool-ex.rs", tool);
    repo.git(&["add", "--all"]);
    repo
}

fn inputs_build(repo: &Repo, command: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--"])
        .args(command)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn builds(arguments: &[&str]) -> Repo {
    let repo = workspace();
    let mut command = vec!["cargo"];
    command.extend_from_slice(arguments);
    command.push("--offline");
    let built = inputs_build(&repo, &command);
    assert!(
        built.status.success(),
        "cargo {arguments:?}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    repo
}

#[test]
fn a_real_cargo_build_of_the_workspace_is_not_refused() {
    builds(&["build"]);
}

#[test]
fn a_real_cargo_build_of_a_package_with_a_binary_and_a_library_is_not_refused() {
    builds(&["build", "-p", "my-app", "--bin", "my-app"]);
}

#[test]
fn a_real_cargo_build_of_a_package_with_only_a_binary_is_not_refused() {
    builds(&["build", "-p", "my-tool", "--bin", "my-tool"]);
}

#[test]
fn a_real_cargo_build_of_the_examples_of_a_package_with_only_a_binary_is_not_refused() {
    builds(&["build", "-p", "my-tool", "--examples"]);
}

#[test]
fn a_real_cargo_build_of_the_examples_of_a_package_with_a_library_is_not_refused() {
    builds(&["build", "-p", "my-app", "--examples"]);
}

#[test]
fn a_real_cargo_build_of_every_target_is_not_refused() {
    builds(&["build", "--all-targets"]);
}

#[test]
fn a_real_cargo_release_build_is_not_refused() {
    builds(&["build", "--release"]);
}

#[test]
fn a_real_cargo_test_build_is_not_refused() {
    builds(&["test", "--no-run"]);
}

#[test]
fn a_real_cargo_check_is_not_refused() {
    builds(&["check"]);
}

fn first_directory(parent: &Path) -> PathBuf {
    let mut directories: Vec<PathBuf> = fs::read_dir(parent)
        .unwrap_or_else(|e| unreachable!("{}: {e}", parent.display()))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")).path())
        .collect();
    directories.sort();
    directories
        .into_iter()
        .next()
        .unwrap_or_else(|| unreachable!("{}", parent.display()))
}

#[test]
fn an_output_no_unit_of_a_real_build_owns_makes_the_next_build_refuse() {
    let repo = builds(&["build", "--all-targets"]);
    let debug = repo.root().join("target/debug");
    let hash = first_directory(&debug.join("build/my-macro"))
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| unreachable!())
        .to_owned();
    let orphans = [
        format!("build/my-macro/{hash}/out/libzzz.rlib"),
        format!("build/my-macro/{hash}/fingerprint/libzzz.rlib"),
        format!("build/my-macro/{hash}/fingerprint/zzz.d"),
        "build/ghost/0123456789abcdef/out/libghost-0123456789abcdef.rlib".to_owned(),
        "libapp.rlib".to_owned(),
        "libzzz.d".to_owned(),
        "zzz.d".to_owned(),
        "examples/libzzz.rlib".to_owned(),
    ];
    for orphan in orphans {
        let path = debug.join(&orphan);
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(&path, "").unwrap_or_else(|e| unreachable!("{e}"));
        let refused = inputs_build(&repo, &["true"]);
        fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(refused.status.code(), Some(1), "{orphan}: {refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains("found no units in") && stderr.contains("unknown cargo layout?"),
            "{orphan}: {stderr}"
        );
    }
    let accepted = inputs_build(&repo, &["true"]);
    assert!(accepted.status.success(), "{accepted:?}");
}
