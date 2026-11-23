// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::support::fresh_dir;

type Tree = BTreeMap<String, Vec<u8>>;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn collect_files(dir: &Path, root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.path().is_dir() {
            collect_files(&entry.path(), root, found);
        } else if let Ok(relative) = entry.path().strip_prefix(root) {
            found.push(relative.to_path_buf());
        }
    }
}

fn tree(dir: &Path) -> Tree {
    let mut found = Vec::new();
    collect_files(dir, dir, &mut found);
    found
        .into_iter()
        .map(|file| {
            let name = file.to_string_lossy().into_owned();
            let path = name.strip_suffix(".fixture").unwrap_or(&name).to_owned();
            (
                path,
                fs::read(dir.join(&file)).unwrap_or_else(|e| unreachable!("{e}")),
            )
        })
        .collect()
}

fn import(repo: &Path, commits: &[Tree]) {
    let mut stream = Vec::new();
    let mut mark = 0;
    let mut parent = None;
    for files in commits {
        mark += 1;
        let commit = mark;
        let mut modify = Vec::new();
        for (path, bytes) in files {
            mark += 1;
            stream.extend_from_slice(
                format!("blob\nmark :{mark}\ndata {}\n", bytes.len()).as_bytes(),
            );
            stream.extend_from_slice(bytes);
            stream.push(b'\n');
            modify.push(format!("M 100644 :{mark} {path}\n"));
        }
        stream.extend_from_slice(format!("commit refs/heads/main\nmark :{commit}\n").as_bytes());
        stream.extend_from_slice(b"committer t <t@example.com> 0 +0000\ndata 4\nplan\n");
        if let Some(parent) = parent {
            stream.extend_from_slice(format!("from :{parent}\n").as_bytes());
        }
        stream.extend_from_slice(b"deleteall\n");
        stream.extend_from_slice(modify.concat().as_bytes());
        stream.push(b'\n');
        parent = Some(commit);
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    child
        .stdin
        .take()
        .unwrap_or_else(|| unreachable!("fast-import stdin"))
        .write_all(&stream)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        output.status.success(),
        "fast-import: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn repository(
    case: &Path,
    event: &str,
    name: &str,
) -> (velnor_test_support::ScratchDir, String, String) {
    repository_with(case, event, name, "")
}

pub(crate) fn repository_with(
    case: &Path,
    event: &str,
    name: &str,
    config_tail: &str,
) -> (velnor_test_support::ScratchDir, String, String) {
    let repo = fresh_dir("velnor-ci-plan-diff", name);
    git(&repo, &["init", "-q", "-b", "main"]);
    let snapshot = case.join("snapshot");
    let mut common = tree(&snapshot.join("common"));
    if !config_tail.is_empty() {
        let config = common
            .get_mut(".velnor/config.toml")
            .unwrap_or_else(|| unreachable!("no .velnor/config.toml in common"));
        config.extend_from_slice(config_tail.as_bytes());
    }
    let mut base = common.clone();
    base.extend(tree(&snapshot.join("base-only")));
    let mut commits = vec![base];
    if event != "local" {
        common.extend(tree(&snapshot.join("head-only")));
        commits.push(common);
    }
    import(&repo, &commits);
    let last: Vec<(&String, &Vec<u8>)> = commits
        .last()
        .unwrap_or_else(|| unreachable!("no commits"))
        .iter()
        .collect();
    std::thread::scope(|scope| {
        for chunk in last.chunks(last.len() / 8 + 1) {
            let repo = &repo;
            scope.spawn(move || {
                for (path, bytes) in chunk {
                    let file = repo.join(path);
                    fs::create_dir_all(file.parent().unwrap_or(repo))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::write(file, bytes).unwrap_or_else(|e| unreachable!("{e}"));
                }
            });
        }
    });
    git(&repo, &["read-tree", "HEAD"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let base = if event == "local" {
        head.clone()
    } else {
        git(&repo, &["rev-parse", "HEAD^"])
    };
    (repo, base, head)
}
