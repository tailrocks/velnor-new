// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use crate::io::Repo;

fn index_raw(repo: &Repo, path: &[u8]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success());
    let mut entry =
        format!("100644,{},", String::from_utf8_lossy(&output.stdout).trim()).into_bytes();
    entry.extend_from_slice(path);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(OsStr::from_bytes(&entry))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
}

fn inputs(root: &Path, operation: &str, target: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", operation, "--target-dir"])
        .arg(target)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn raw_tracked_paths_outside_a_root_leave_its_safe_snapshot_usable() {
    let repo = Repo::new("raw-outside-root");
    repo.write("app/src/lib.rs", "pub fn one() {}\n");
    index_raw(&repo, b"other/caf\xe9.rs");
    let built = repo.mtime("app/src/lib.rs");
    let app = repo.root().join("app");
    let saved = inputs(&app, "save", Path::new("target"));
    assert!(saved.status.success(), "{saved:?}");
    assert!(
        app.join("target")
            .join(velnor_ci::inputs::inputs_file("app/"))
            .is_file()
    );
    repo.set_mtime("app/src/lib.rs", built + Duration::from_secs(10));
    let restored = inputs(&app, "restore", Path::new("target"));
    assert!(restored.status.success(), "{restored:?}");
    assert_eq!(repo.mtime("app/src/lib.rs"), built);
}

#[test]
fn raw_tracked_paths_inside_a_target_preserve_checkout_and_cache() {
    for linked in [false, true] {
        let repo = Repo::new("raw-inside-target");
        repo.write("src/lib.rs", "pub fn one() {}\n");
        repo.write_unstaged("cache/marker", "keep cached artifact\n");
        repo.write_unstaged("cache/.velnor/inputs.tsv", "invalid\n");
        index_raw(&repo, b"cache/caf\xe9.rs");
        let target = if linked {
            std::os::unix::fs::symlink("cache", repo.root().join("target"))
                .unwrap_or_else(|e| unreachable!("{e}"));
            Path::new("target")
        } else {
            Path::new("cache")
        };
        for operation in ["save", "restore"] {
            let output = inputs(repo.root(), operation, target);
            assert!(
                !output.status.success(),
                "{operation} linked={linked}: {output:?}"
            );
            assert_eq!(
                std::fs::read_to_string(repo.root().join("src/lib.rs")).unwrap_or_default(),
                "pub fn one() {}\n"
            );
            assert_eq!(
                std::fs::read_to_string(repo.root().join("cache/marker")).unwrap_or_default(),
                "keep cached artifact\n"
            );
            assert_eq!(
                std::fs::read_to_string(repo.root().join("cache/.velnor/inputs.tsv"))
                    .unwrap_or_default(),
                "invalid\n"
            );
            if linked {
                assert_eq!(
                    std::fs::read_link(repo.root().join("target")).ok(),
                    Some(Path::new("cache").to_path_buf())
                );
            }
        }
    }
}
