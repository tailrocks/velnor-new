// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

fn run(root: &Path, program: &Path, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "{} {args:?}: {text}",
        program.display()
    );
    text
}

#[test]
fn a_malformed_snapshot_discards_old_cargo_artifacts_before_a_backdated_edit_builds() {
    let repo = Repo::new("malformed-snapshot-cargo");
    repo.write_all(&[
        (
            "Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("src/main.rs", "fn main() { println!(\"A\"); }\n"),
    ]);
    let cargo = Path::new(env!("CARGO"));
    let helper = Path::new(env!("CARGO_BIN_EXE_velnor-ci"));
    run(repo.root(), cargo, &["build", "--offline", "-v"]);
    let binary = repo.root().join("target/debug/app");
    assert_eq!(run(repo.root(), &binary, &[]).trim(), "A");
    run(repo.root(), helper, &["inputs", "save"]);
    std::fs::write(
        repo.root().join("target/.velnor/inputs.tsv"),
        "invalid snapshot\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("src/main.rs", "fn main() { println!(\"B\"); }\n");
    repo.set_mtime(
        "src/main.rs",
        UNIX_EPOCH + Duration::from_secs(1_600_000_000),
    );
    let restored = run(repo.root(), helper, &["inputs", "restore"]);
    assert!(
        !repo.root().join("target").exists(),
        "target survived: {restored}"
    );
    assert!(restored.contains("building cold"), "{restored}");
    run(repo.root(), helper, &["restored"]);
    let built = run(repo.root(), cargo, &["build", "--offline", "-v"]);
    assert!(built.contains("Compiling app "), "{built}");
    assert_eq!(run(repo.root(), &binary, &[]).trim(), "B");
}

#[test]
fn a_missing_snapshot_discards_old_cargo_artifacts_before_a_backdated_edit_builds() {
    let repo = Repo::new("missing-snapshot-fresh-checkout");
    repo.write_all(&[
        (
            "Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("src/main.rs", "fn main() { println!(\"A\"); }\n"),
    ]);
    let cargo = Path::new(env!("CARGO"));
    let helper = Path::new(env!("CARGO_BIN_EXE_velnor-ci"));
    run(repo.root(), cargo, &["build", "--offline", "-v"]);
    let binary = repo.root().join("target/debug/app");
    assert_eq!(run(repo.root(), &binary, &[]).trim(), "A");
    let checkout = UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    repo.write_unstaged("src/main.rs", "fn main() { println!(\"B\"); }\n");
    for path in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
        repo.set_mtime(path, checkout);
    }
    let checkout = repo.mtime("src/main.rs");
    assert!(!repo.root().join("target/.velnor/inputs.tsv").exists());
    let restored = run(repo.root(), helper, &["inputs", "restore"]);
    assert!(!binary.exists(), "target survived: {restored}");
    assert!(restored.contains("building cold"), "{restored}");
    assert_eq!(repo.mtime("src/main.rs"), checkout);
    run(repo.root(), helper, &["restored"]);
    let built = run(repo.root(), cargo, &["build", "--offline", "-v"]);
    assert!(
        built.contains("Compiling app ") || built.contains("Dirty app "),
        "{built}"
    );
    assert!(!built.contains("Fresh app "), "{built}");
    assert_eq!(run(repo.root(), &binary, &[]).trim(), "B");
}

#[test]
fn a_retargeted_source_link_discards_old_cargo_artifacts_before_building() {
    let repo = Repo::new("retargeted-link-cargo");
    repo.write_all(&[
        (
            "Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("src/a.rs", "fn main() { println!(\"A\"); }\n"),
        ("src/b.rs", "fn main() { println!(\"B\"); }\n"),
    ]);
    repo.link("a.rs", "src/main.rs");
    let cargo = Path::new(env!("CARGO"));
    let helper = Path::new(env!("CARGO_BIN_EXE_velnor-ci"));
    run(repo.root(), cargo, &["build", "--offline", "-v"]);
    let binary = repo.root().join("target/debug/app");
    assert_eq!(run(repo.root(), &binary, &[]).trim(), "A");
    run(repo.root(), helper, &["inputs", "save"]);
    std::fs::remove_file(repo.root().join("src/main.rs")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.link("b.rs", "src/main.rs");
    repo.set_mtime("src/b.rs", UNIX_EPOCH + Duration::from_secs(1_600_000_000));
    let restored = run(repo.root(), helper, &["inputs", "restore"]);
    assert!(
        !repo.root().join("target").exists(),
        "target survived: {restored}"
    );
    assert!(restored.contains("building cold"), "{restored}");
    run(repo.root(), helper, &["restored"]);
    let built = run(repo.root(), cargo, &["build", "--offline", "-v"]);
    assert!(built.contains("Compiling app "), "{built}");
    assert_eq!(run(repo.root(), &binary, &[]).trim(), "B");
}
