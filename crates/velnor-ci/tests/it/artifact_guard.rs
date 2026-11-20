// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_ci::artifact_guard::sweep;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "velnor-ci-artifact-guard-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
        Self(path)
    }

    fn root(&self) -> &Path {
        &self.0
    }

    fn write_unstaged(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Site {
    repo: Dir,
    outside: Dir,
}

fn site(name: &str) -> Site {
    let repo = Dir::new(name);
    let outside = Dir::new(&format!("{name}-outside"));
    outside.write_unstaged("credentials", "token\n");
    outside.write_unstaged("dir/secret.log", "token\n");
    Site { repo, outside }
}

fn link(target: &Path, at: &Path) {
    fs::create_dir_all(at.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink(target, at).unwrap_or_else(|e| unreachable!("{e}"));
}

fn swept(site: &Site, patterns: &[&str]) -> Vec<PathBuf> {
    let patterns: Vec<String> = patterns.iter().map(|path| (*path).to_owned()).collect();
    sweep(site.repo.root(), &patterns)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .into_iter()
        .collect()
}

fn exists(site: &Site, path: &str) -> bool {
    fs::symlink_metadata(site.repo.root().join(path)).is_ok()
}

#[test]
fn a_link_to_a_file_outside_the_workspace_inside_an_artifact_directory_is_removed() {
    let site = site("guard-file-link");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("logs/leak.log"),
    );
    assert_eq!(swept(&site, &["logs/"]), [PathBuf::from("logs/leak.log")]);
    assert!(!exists(&site, "logs/leak.log"));
    assert!(exists(&site, "logs/real.log"));
    assert!(site.outside.root().join("credentials").is_file());
}

#[test]
fn a_declared_path_that_is_a_link_to_an_outside_directory_is_removed() {
    let site = site("guard-directory-link");
    link(
        &site.outside.root().join("dir"),
        &site.repo.root().join("logs"),
    );
    assert_eq!(swept(&site, &["logs/"]), [PathBuf::from("logs")]);
    assert!(!exists(&site, "logs"));
    assert!(site.outside.root().join("dir/secret.log").is_file());
}

#[test]
fn a_link_in_the_middle_of_a_glob_is_removed_before_the_glob_can_follow_it() {
    let site = site("guard-middle-link");
    link(
        &site.outside.root().join("dir"),
        &site.repo.root().join("reports/escape"),
    );
    site.repo.write_unstaged("reports/real/a.log", "ok\n");
    assert_eq!(
        swept(&site, &["reports/*/a.log", "reports/**/*.log"]),
        [PathBuf::from("reports/escape")]
    );
    assert!(exists(&site, "reports/real/a.log"));
    assert!(site.outside.root().join("dir/secret.log").is_file());
}

#[test]
fn a_link_to_a_file_inside_the_workspace_is_removed_too() {
    let site = site("guard-inside-link");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    link(
        &site.repo.root().join("logs/real.log"),
        &site.repo.root().join("logs/alias.log"),
    );
    assert_eq!(
        swept(&site, &["logs/*.log"]),
        [PathBuf::from("logs/alias.log")]
    );
    assert!(exists(&site, "logs/real.log"));
}

#[test]
fn paths_with_no_link_and_links_outside_the_declared_paths_are_left_alone() {
    let site = site("guard-clean");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    site.repo.write_unstaged("junit.xml", "<x/>\n");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("other/leak"),
    );
    assert_eq!(
        swept(&site, &["logs/", "junit.xml", "missing/**/*.log"]),
        Vec::<PathBuf>::new()
    );
    assert!(exists(&site, "other/leak"));
    assert!(exists(&site, "logs/real.log"));
    assert!(exists(&site, "junit.xml"));
}

#[test]
fn the_command_warns_once_per_removed_link_without_forging_a_workflow_command() {
    let site = site("guard-command");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("logs/x\n::error::FORGED"),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(site.outside.root())
        .env("GITHUB_WORKSPACE", site.repo.root())
        .args([
            "artifacts",
            "check",
            "--path",
            "logs/",
            "--path",
            "junit.xml",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().filter(|line| line.starts_with("::")).count(),
        1,
        "{stdout}"
    );
    assert!(
        stdout.contains("::warning::") && stdout.contains("%0A::error::FORGED"),
        "{stdout}"
    );
    assert!(site.outside.root().join("credentials").is_file());
}
