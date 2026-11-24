// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;

use super::{unit, velnor_ci};
use crate::io::Repo;

const BUILD_SCRIPT: &str = r#"use std::{env, fs, path::Path};

fn main() {
    let out = env::var("OUT_DIR").unwrap();
    let nested = Path::new(&out).join("generated/deps");
    fs::create_dir_all(&nested).unwrap();
    for file in ["gen.d", "gen-0123456789abcdef.d", "generated/object.d", "generated/deps/dep-0123456789abcdef.d"] {
        fs::write(Path::new(&out).join(file), "").unwrap();
    }
}
"#;

const PROFILES: [&str; 4] = ["examples", "deps", "out", "fingerprint"];

fn workspace() -> Repo {
    let repo = Repo::new("audit-real");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    unit(
        &repo,
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [\"my-app\", \"my-lib\", \"my-macro\", \"my-tool\"]\nresolver = \"3\"\n{}",
            PROFILES
                .map(|name| format!("\n[profile.{name}]\ninherits = \"dev\"\n"))
                .concat()
        ),
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
    unit(&repo, "my-app/build.rs", BUILD_SCRIPT);
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

fn inputs_build(repo: &Repo, new_layout: bool, command: &[&str]) -> Output {
    let mut build = velnor_ci();
    build.env("RUSTC_BOOTSTRAP", "1").env(
        "CARGO_UNSTABLE_BUILD_DIR_NEW_LAYOUT",
        new_layout.to_string(),
    );
    build
        .current_dir(repo.root())
        .args(["inputs", "build", "--"])
        .args(command)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn build_every_shape(repo: &Repo, new_layout: bool) {
    let mut scenarios: Vec<Vec<&str>> = vec![
        vec!["build", "-p", "my-tool", "--bin", "my-tool"],
        vec!["build", "-p", "my-tool", "--examples"],
        vec!["build", "-p", "my-app", "--bin", "my-app"],
        vec!["build"],
        vec!["build", "--all-targets"],
        vec!["test", "--no-run"],
        vec!["check"],
        vec!["build", "--release"],
    ];
    scenarios.extend(PROFILES.map(|profile| vec!["build", "--all-targets", "--profile", profile]));
    for arguments in scenarios {
        let mut command = vec!["cargo"];
        command.extend_from_slice(&arguments);
        command.push("--offline");
        let built = inputs_build(repo, new_layout, &command);
        assert!(
            built.status.success(),
            "cargo {arguments:?}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
    }
}

fn orphans(configuration: &str) -> Vec<String> {
    let mut orphans: Vec<String> = [
        "libzzz.rlib",
        "libzzz.d",
        "zzz.d",
        "zzz-0123456789abcdef.d",
        "examples/zzz.d",
        "examples/libzzz.rlib",
        "deps/libzzz-0123456789abcdef.rlib",
        "build/zzz/0123456789abcdef/out/libzzz-0123456789abcdef.rlib",
    ]
    .iter()
    .map(|orphan| format!("{configuration}/{orphan}"))
    .collect();
    orphans.push("zzz-0123456789abcdef.d".to_owned());
    orphans
}

fn plant_orphans(repo: &Repo, new_layout: bool) {
    for configuration in ["debug", "release", "examples", "deps", "out", "fingerprint"] {
        for orphan in orphans(configuration) {
            let path = repo.root().join("target").join(&orphan);
            fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
                .unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(&path, "").unwrap_or_else(|e| unreachable!("{e}"));
            let refused = inputs_build(repo, new_layout, &["true"]);
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
            assert_eq!(refused.status.code(), Some(1), "{orphan}: {refused:?}");
            let stderr = String::from_utf8_lossy(&refused.stderr);
            assert!(
                stderr.contains("found no units in") && stderr.contains("unknown cargo layout?"),
                "{orphan}: {stderr}"
            );
        }
    }
    let accepted = inputs_build(repo, new_layout, &["true"]);
    assert!(accepted.status.success(), "{accepted:?}");
}

fn real_build_then_orphans(new_layout: bool) {
    let repo = workspace();
    build_every_shape(&repo, new_layout);
    let debug = repo.root().join("target/debug");
    assert_eq!(debug.join("build/my-lib").is_dir(), new_layout);
    assert_eq!(debug.join("deps").is_dir(), !new_layout);
    plant_orphans(&repo, new_layout);
}

#[test]
fn a_real_cargo_build_in_the_legacy_layout_is_not_refused_and_an_output_no_unit_owns_is() {
    real_build_then_orphans(false);
}

#[test]
fn a_real_cargo_build_in_the_new_layout_is_not_refused_and_an_output_no_unit_owns_is() {
    real_build_then_orphans(true);
}
