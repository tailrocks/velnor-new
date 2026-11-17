// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

fn inputs(repo: &Repo, root: &str, operation: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join(root))
        .args(["inputs", operation, "--target-dir", "../target"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    text
}

fn snapshot(repo: &Repo, root: &str) -> std::path::PathBuf {
    repo.root()
        .join("target")
        .join(velnor_ci::inputs::inputs_file(&format!("{root}/")))
}

fn roots(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust='1.99.1'\n"),
        (
            "app/Cargo.toml",
            "[package]\nname='app-pkg'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("app/src/lib.rs", "pub fn app() {}\n"),
        (
            "other/Cargo.toml",
            "[package]\nname='other-pkg'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("other/src/lib.rs", "pub fn other() {}\n"),
    ]);
    repo
}

#[test]
fn invalid_symlinks_only_invalidate_their_own_root_snapshot() {
    let repo = roots("input-link-scope");
    let built = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for root in ["app", "other"] {
        repo.set_mtime(&format!("{root}/src/lib.rs"), built);
        inputs(&repo, root, "save");
        assert!(snapshot(&repo, root).is_file());
    }
    repo.link("lib.rs", "other/odd\tlink");
    inputs(&repo, "app", "save");
    repo.set_mtime("app/src/lib.rs", checkout);
    inputs(&repo, "app", "restore");
    assert_eq!(repo.mtime("app/src/lib.rs"), built);
    assert!(snapshot(&repo, "app").is_file());
    assert!(inputs(&repo, "other", "save").contains("saved nothing: 1 tracked symlinks"));
    assert!(!snapshot(&repo, "other").exists());
    repo.set_mtime("other/src/lib.rs", checkout);
    inputs(&repo, "other", "restore");
    assert_eq!(repo.mtime("other/src/lib.rs"), checkout);
    inputs(&repo, "app", "save");
    assert!(snapshot(&repo, "app").is_file());
    repo.link("lib.rs", "app/odd\tlink");
    assert!(inputs(&repo, "app", "save").contains("saved nothing: 1 tracked symlinks"));
    assert!(!snapshot(&repo, "app").exists());
}

#[test]
fn recorded_external_inputs_follow_root_owners_and_sibling_dependencies() {
    let repo = roots("input-owner-scope");
    repo.write_all(&[
        ("app/Cargo.toml", "[package]\nname='app-pkg'\nversion='0.1.0'\nedition='2024'\n[dependencies]\ndep-pkg={path='../dep'}\n"),
        ("dep/Cargo.toml", "[package]\nname='dep-pkg'\nversion='0.1.0'\nedition='2024'\n"),
        ("dep/src/lib.rs", "pub fn dep() {}\n"),
        ("shared.txt", "app input\n"),
        ("notes.txt", "other input\n"),
        ("dep-input.txt", "dependency input\n"),
        (".velnor/rust-inputs.lock", "shared.txt\tapp-pkg\nnotes.txt\tother-pkg\ndep-input.txt\tdep-pkg\n"),
    ]);
    for root in ["app", "other", "dep"] {
        let output = Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(repo.root().join(root))
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    let built = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    let paths = ["shared.txt", "notes.txt", "dep-input.txt", "dep/src/lib.rs"];
    for root in ["app", "other"] {
        for path in paths {
            repo.set_mtime(path, built);
        }
        inputs(&repo, root, "save");
        for path in paths {
            repo.set_mtime(path, checkout);
        }
        inputs(&repo, root, "restore");
        let app = root == "app";
        assert_eq!(repo.mtime("shared.txt"), if app { built } else { checkout });
        assert_eq!(repo.mtime("notes.txt"), if app { checkout } else { built });
        assert_eq!(
            repo.mtime("dep-input.txt"),
            if app { built } else { checkout }
        );
        assert_eq!(
            repo.mtime("dep/src/lib.rs"),
            if app { built } else { checkout }
        );
    }
    repo.write(".velnor/rust-inputs.lock", ".\tother-pkg\n");
    inputs(&repo, "app", "save");
    let saved = fs::read_to_string(snapshot(&repo, "app")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!saved.contains("notes.txt\t"), "{saved}");
}

#[test]
fn listed_external_inputs_restore_without_recorded_metadata() {
    let repo = roots("input-owner-fallback");
    repo.write_all(&[
        ("shared.txt", "input\n"),
        (".velnor/rust-inputs.lock", "shared.txt\tother-pkg\n"),
    ]);
    let built = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("shared.txt", built);
    inputs(&repo, "app", "save");
    repo.set_mtime("shared.txt", checkout);
    inputs(&repo, "app", "restore");
    assert_eq!(repo.mtime("shared.txt"), built);
    assert!(
        !repo
            .root()
            .join(Path::new(".velnor/rust-metadata.json"))
            .exists()
    );
}
