// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use super::{unit, velnor_ci};
use crate::io::Repo;

const BUILD_SCRIPT: &str = r#"use std::{env, fs, path::Path};

fn main() {
    let out = env::var("OUT_DIR").unwrap();
    for file in ["gen.rlib", "gen.d", "libgen-0123456789abcdef.rmeta", "gen-0123456789abcdef.d"] {
        fs::write(Path::new(&out).join(file), "").unwrap();
    }
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    if Path::new(&manifest).join("FAIL").exists() {
        std::process::exit(1);
    }
}
"#;

fn workspace() -> Repo {
    let repo = Repo::new("audit-real");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    unit(
        &repo,
        "Cargo.toml",
        "[workspace]\nmembers = [\"my-app\", \"my-lib\", \"my-macro\", \"my-tool\"]\nresolver = \"3\"\n\n[profile.examples]\ninherits = \"dev\"\n",
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

fn inputs_build(repo: &Repo, command: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--"])
        .args(command)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

const DEBUG: [&[&str]; 7] = [
    &["build", "-p", "my-tool", "--bin", "my-tool"],
    &["build", "-p", "my-tool", "--examples"],
    &["build", "-p", "my-app", "--bin", "my-app"],
    &["build", "-p", "my-app", "--examples"],
    &["build"],
    &["build", "--all-targets"],
    &["test", "--no-run"],
];

const OTHER_PROFILES: [&[&str]; 3] = [
    &["check"],
    &["build", "--release"],
    &["build", "--profile", "examples", "-p", "my-lib"],
];

fn build_in_one_target(repo: &Repo, scenarios: &[&[&str]]) {
    for arguments in scenarios {
        let mut command = vec!["cargo"];
        command.extend_from_slice(arguments);
        command.push("--offline");
        let built = inputs_build(repo, &command);
        assert!(
            built.status.success(),
            "cargo {arguments:?}: {}",
            String::from_utf8_lossy(&built.stderr)
        );
    }
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

fn plant_orphans_beside_a_real_build(repo: &Repo) {
    let debug = repo.root().join("target/debug");
    let hash = first_directory(&debug.join("build/my-macro"))
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| unreachable!())
        .to_owned();
    let orphans = [
        format!("build/my-macro/{hash}/out/libzzz.rlib"),
        format!("build/my-macro/{hash}/out/invoked.timestamp"),
        format!("build/my-macro/{hash}/fingerprint/libzzz.rlib"),
        format!("build/my-macro/{hash}/fingerprint/zzz.d"),
        "build/ghost/0123456789abcdef/out/libghost-0123456789abcdef.rlib".to_owned(),
        "build/zzz-0123456789abcdef/out/x.rlib".to_owned(),
        "build/zzz-0123456789abcdef/out/x.d".to_owned(),
        "libapp.rlib".to_owned(),
        "libzzz.d".to_owned(),
        "zzz.d".to_owned(),
        "invoked.timestamp".to_owned(),
        "deps/invoked.timestamp".to_owned(),
        "examples/libzzz.rlib".to_owned(),
    ];
    for orphan in orphans {
        let path = debug.join(&orphan);
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(&path, "").unwrap_or_else(|e| unreachable!("{e}"));
        let refused = inputs_build(repo, &["true"]);
        fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(refused.status.code(), Some(1), "{orphan}: {refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains("found no units in") && stderr.contains("unknown cargo layout?"),
            "{orphan}: {stderr}"
        );
    }
    let accepted = inputs_build(repo, &["true"]);
    assert!(accepted.status.success(), "{accepted:?}");
}

fn a_failed_build_script_does_not_block_the_retry(repo: &Repo) {
    let fail = repo.root().join("my-app/FAIL");
    fs::write(&fail, "").unwrap_or_else(|e| unreachable!("{e}"));
    let failed = inputs_build(repo, &["cargo", "build", "-p", "my-app", "--offline"]);
    assert!(!failed.status.success(), "{failed:?}");
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        stderr.contains("failed to run custom build command"),
        "{stderr}"
    );
    fs::remove_file(&fail).unwrap_or_else(|e| unreachable!("{e}"));
    build_in_one_target(
        repo,
        &[&["build", "-p", "my-app"], &["build", "--all-targets"]],
    );
}

fn an_empty_target_dir_is_left_to_cargo_to_reject(repo: &Repo) {
    for arguments in [
        &["inputs", "build", "--", "cargo", "build", "--offline"][..],
        &["inputs", "audit"],
    ] {
        let refused = velnor_ci()
            .current_dir(repo.root())
            .env("CARGO_TARGET_DIR", "")
            .args(arguments)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(refused.status.code(), Some(1), "{arguments:?}: {refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains("the target directory is set to an empty string"),
            "{arguments:?}: {stderr}"
        );
    }
}

#[test]
fn a_real_cargo_build_is_not_refused_and_an_output_no_unit_of_it_owns_is() {
    std::thread::scope(|scope| {
        let debug = scope.spawn(|| build_in_one_target(&workspace(), &DEBUG));
        let other = scope.spawn(|| build_in_one_target(&workspace(), &OTHER_PROFILES));
        let repo = workspace();
        a_failed_build_script_does_not_block_the_retry(&repo);
        an_empty_target_dir_is_left_to_cargo_to_reject(&repo);
        plant_orphans_beside_a_real_build(&repo);
        for handle in [debug, other] {
            handle
                .join()
                .unwrap_or_else(|e| std::panic::resume_unwind(e));
        }
    });
}
