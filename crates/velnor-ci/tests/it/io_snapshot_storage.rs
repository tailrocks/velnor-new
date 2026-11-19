// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use crate::io::Repo;

fn helper(root: &Path, operation: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", operation])
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn saving_replaces_snapshot_file_links_without_changing_checkout_files() {
    let mut failures = Vec::new();
    for hardlink in [false, true] {
        let repo = Repo::new("snapshot-file-link");
        let source = "pub fn value() -> u8 { 7 }\n";
        repo.write("src/lib.rs", source);
        let before = repo.mtime("src/lib.rs");
        let index =
            fs::read(repo.root().join(".git/index")).unwrap_or_else(|e| unreachable!("{e}"));
        let snapshot = repo.root().join("target/.velnor/inputs.tsv");
        fs::create_dir_all(snapshot.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let tracked = repo.root().join("src/lib.rs");
        if hardlink {
            fs::hard_link(&tracked, &snapshot).unwrap_or_else(|e| unreachable!("{e}"));
        } else {
            symlink(&tracked, &snapshot).unwrap_or_else(|e| unreachable!("{e}"));
        }
        let saved = helper(repo.root(), "save");
        if !saved.status.success()
            || fs::read_to_string(&tracked).unwrap_or_default() != source
            || fs::read(repo.root().join(".git/index")).unwrap_or_default() != index
        {
            failures.push(format!(
                "hardlink={hardlink}: save changed checkout: {saved:?}"
            ));
            continue;
        }
        let metadata = fs::symlink_metadata(&snapshot).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(metadata.is_file());
        assert_ne!(
            metadata.ino(),
            fs::metadata(&tracked)
                .unwrap_or_else(|e| unreachable!("{e}"))
                .ino()
        );
        repo.set_mtime("src/lib.rs", before + Duration::from_secs(10));
        let restored = helper(repo.root(), "restore");
        assert!(restored.status.success(), "{restored:?}");
        assert_eq!(repo.mtime("src/lib.rs"), before);
        assert_eq!(fs::read_to_string(tracked).unwrap_or_default(), source);
        assert_eq!(
            fs::read(repo.root().join(".git/index")).unwrap_or_default(),
            index
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn saving_refuses_snapshot_directory_links_before_writing_or_removing_files() {
    let mut failures = Vec::new();
    for unverifiable in [false, true] {
        let repo = Repo::new("snapshot-directory-link");
        let source = "tracked snapshot referent\n";
        repo.write_all(&[
            ("src/lib.rs", "pub fn value() {}\n"),
            ("inputs.tsv", source),
        ]);
        if unverifiable {
            symlink("unrepresentable\n", repo.root().join("tracked-link"))
                .unwrap_or_else(|e| unreachable!("{e}"));
            repo.git(&["add", "--all"]);
        }
        let index =
            fs::read(repo.root().join(".git/index")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::create_dir_all(repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
        symlink(repo.root(), repo.root().join("target/.velnor"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let output = helper(repo.root(), "save");
        if output.status.success()
            || fs::read_to_string(repo.root().join("inputs.tsv")).unwrap_or_default() != source
            || fs::read(repo.root().join(".git/index")).unwrap_or_default() != index
        {
            failures.push(format!(
                "unverifiable={unverifiable}: snapshot parent changed checkout: {output:?}"
            ));
        }
        assert_eq!(
            fs::read_to_string(repo.root().join("src/lib.rs")).unwrap_or_default(),
            "pub fn value() {}\n"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
