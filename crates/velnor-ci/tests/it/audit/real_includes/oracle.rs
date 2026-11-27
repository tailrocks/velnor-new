// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_rust::{Located, dep_info_paths, locate, rerun_if_changed};

use super::super::{audit, velnor_ci};
use crate::io::Repo;

#[derive(Clone, Copy)]
pub(super) enum Share {
    Zero,
    Shared,
}

#[derive(Clone, Copy)]
pub(super) enum Layout {
    Legacy,
    New,
}

pub(super) fn build(repo: &Repo, share: Share, layout: Layout) {
    for arguments in [&["build", "--all-targets"][..], &["check"][..]] {
        let mut command = velnor_ci();
        command
            .env("RUSTC_BOOTSTRAP", "1")
            .env("MBX_BUILD_SCRIPT_EXECUTION", "0")
            .env(
                "CARGO_UNSTABLE_BUILD_DIR_NEW_LAYOUT",
                matches!(layout, Layout::New).to_string(),
            );
        match share {
            Share::Zero => command.env("MBX_SHARE_OUT_DIR", "0"),
            Share::Shared => command.env("MBX_SHARE_OUT_DIR", "1"),
        };
        let output = command
            .current_dir(repo.root())
            .args(["inputs", "build", "--", "cargo"])
            .args(arguments)
            .arg("--offline")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub(super) fn audit_written(repo: &Repo) {
    let written = audit(repo, &["--write"]);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
}

fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn owner_of(file: &Path) -> String {
    let folder = file
        .ancestors()
        .find_map(|ancestor| {
            let parent = ancestor.parent()?;
            (parent.file_name()? == "build").then(|| {
                ancestor
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })?
        })
        .unwrap_or_default();
    folder
        .rsplit_once('-')
        .map_or(folder.clone(), |(package, _)| package.to_owned())
}

pub(super) fn named_reads(repo: &Repo) -> BTreeSet<String> {
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let mut files = Vec::new();
    walk(&root.join("target"), &mut files);
    let mut named = BTreeSet::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let listed: Vec<(String, PathBuf)> = if file.extension().is_some_and(|e| e == "d") {
            dep_info_paths(&text)
                .into_iter()
                .map(|path| (path, root.clone()))
                .collect()
        } else if name == "output" || file.ends_with("run/stdout") {
            let base = root.join("crates").join(owner_of(&file));
            rerun_if_changed(&text)
                .into_iter()
                .map(|path| (path, base.clone()))
                .collect()
        } else {
            continue;
        };
        for (raw, base) in listed {
            if let Located::Inside(path) = locate(&root, &base, &raw)
                && !path.as_str().starts_with("target/")
            {
                named.insert(path.to_string());
            }
        }
    }
    named
}

pub(super) fn tracked(repo: &Repo) -> BTreeSet<String> {
    let listed = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["ls-files"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

pub(super) fn planned(repo: &Repo, changed: &str) -> Vec<(String, String)> {
    let output = velnor_ci()
        .current_dir(repo.root())
        .env("GITHUB_OUTPUT", repo.root().join(".git/github-output"))
        .env("GITHUB_EVENT_NAME", "pull_request")
        .args(["plan", "--changed", changed, "--explain"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        output.status.success(),
        "{changed}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_file(repo.root().join(".git/github-output"));
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter_map(|line| line.split_once(' '))
        .map(|(package, reason)| (package.to_owned(), reason.to_owned()))
        .collect()
}
