// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::io::Repo;

fn run(root: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{program} {args:?}: {text}");
    text
}

fn inputs(root: &Path, operation: &str, target: Option<&str>) -> String {
    let mut args = vec!["inputs", operation];
    if let Some(target) = target {
        args.extend(["--target-dir", target]);
    }
    run(root, env!("CARGO_BIN_EXE_velnor-ci"), &args)
}

fn cargo(root: &Path) -> String {
    run(root, "cargo", &["build", "--offline", "-v"])
}

fn warm_and_changed(target: Option<&str>) {
    let repo = Repo::new("restore-cargo");
    repo.write_all(&[
        (".gitignore", "**/target/\n/cache dir/\n"),
        (".velnor/rust-ci.toml", "exclude = [\"fixtures/**\", \"other\"]\n"),
        ("app/Cargo.toml", "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nsibling={path='../fixtures/sibling'}\n"),
        ("app/src/main.rs", "fn main() { println!(\"{}\", sibling::value()); }\n"),
        ("fixtures/sibling/Cargo.toml", "[package]\nname='sibling'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nleaf={path='../leaf'}\n"),
        ("fixtures/sibling/src/lib.rs", "pub fn value() -> u8 { leaf::value() }\n"),
        ("fixtures/leaf/Cargo.toml", "[package]\nname='leaf'\nversion='0.1.0'\nedition='2024'\n"),
        ("fixtures/leaf/src/lib.rs", "pub fn value() -> u8 { 1 }\n"),
        ("other/Cargo.toml", "[package]\nname='other'\nversion='0.1.0'\n"),
        ("other/src/lib.rs", "pub fn other() {}\n"),
        ("other/one", "one"),
        ("other/two", "two"),
    ]);
    repo.link("one", "other/link");
    if target.is_some() {
        repo.write(
            "app/.cargo/config.toml",
            "[build]\ntarget-dir='../cache dir'\n",
        );
    }
    let app = repo.root().join("app");
    cargo(&app);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    inputs(&app, "save", target);
    let target_path = app.join(target.unwrap_or("target"));
    assert!(target_path.join(".velnor/inputs.tsv").is_file());
    if target.is_some() {
        assert!(!app.join("target").exists());
    }
    let checkout = SystemTime::now() + Duration::from_secs(10);
    for path in [
        "app/src/main.rs",
        "fixtures/sibling/src/lib.rs",
        "fixtures/leaf/src/lib.rs",
    ] {
        repo.set_mtime(path, checkout);
    }
    std::fs::remove_file(repo.root().join("other/link")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.link("two", "other/link");
    inputs(&app, "restore", target);
    run(&app, env!("CARGO_BIN_EXE_velnor-ci"), &["restored"]);
    let warm = cargo(&app);
    for package in ["app", "sibling", "leaf"] {
        assert!(warm.contains(&format!("Fresh {package} ")), "{warm}");
    }
    assert!(!warm.contains("Compiling "), "{warm}");
    repo.write_unstaged(
        "fixtures/sibling/src/lib.rs",
        "pub fn value() -> u8 { leaf::value() + 1 }\n",
    );
    repo.set_mtime("fixtures/sibling/src/lib.rs", checkout);
    inputs(&app, "restore", target);
    assert_eq!(repo.mtime("fixtures/sibling/src/lib.rs"), checkout);
    let changed = cargo(&app);
    assert!(
        changed.contains("Compiling sibling ") && changed.contains("Compiling app "),
        "{changed}"
    );
    assert!(changed.contains("Fresh leaf "), "{changed}");
    assert_eq!(
        run(
            &app,
            target_path
                .join("debug/app")
                .to_str()
                .unwrap_or_else(|| unreachable!()),
            &[]
        )
        .trim(),
        "2"
    );
}

#[test]
fn nested_cargo_roots_restore_transitive_sibling_sources_and_keep_changes_dirty() {
    warm_and_changed(None);
}

#[test]
fn configured_target_dirs_keep_the_snapshot_with_the_actual_cargo_artifacts() {
    warm_and_changed(Some("../cache dir"));
}

#[test]
fn absolute_target_dirs_restore_times_and_cold_fallback_removes_only_that_target() {
    let repo = Repo::new("absolute-target");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let target = repo.root().join("custom");
    let target = target.to_str().unwrap_or_else(|| unreachable!());
    let built = repo.mtime("src/lib.rs");
    inputs(repo.root(), "save", Some(target));
    repo.set_mtime("src/lib.rs", built + Duration::from_secs(10));
    inputs(repo.root(), "restore", Some(target));
    assert_eq!(repo.mtime("src/lib.rs"), built);
    repo.write_unstaged("target/marker", "keep");
    repo.write_unstaged("custom/debug/marker", "remove");
    repo.write_unstaged("custom/.velnor/inputs.tsv", "invalid\n");
    assert!(inputs(repo.root(), "restore", Some(target)).contains("building cold"));
    assert!(!Path::new(target).exists());
    assert!(repo.root().join("target/marker").is_file());
    for ancestor in [".", ".."] {
        for operation in ["save", "restore"] {
            let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
                .args(["inputs", operation, "--target-dir", ancestor])
                .current_dir(repo.root())
                .output()
                .unwrap_or_else(|e| unreachable!("{e}"));
            assert!(!output.status.success());
            assert!(repo.root().join("src/lib.rs").is_file());
        }
    }
}
