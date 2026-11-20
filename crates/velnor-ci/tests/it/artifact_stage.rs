// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_ci::artifact_stage::{Skip, Staged, stage};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "velnor-ci-artifact-stage-{}-{name}",
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
    staging: Dir,
}

fn site(name: &str) -> Site {
    let repo = Dir::new(name);
    let outside = Dir::new(&format!("{name}-outside"));
    let staging = Dir::new(&format!("{name}-staging"));
    outside.write_unstaged("credentials", "token\n");
    outside.write_unstaged("dir/secret.log", "token\n");
    Site {
        repo,
        outside,
        staging,
    }
}

fn link(target: &Path, at: &Path) {
    fs::create_dir_all(at.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink(target, at).unwrap_or_else(|e| unreachable!("{e}"));
}

fn staged(site: &Site, patterns: &[&str]) -> Staged {
    let patterns: Vec<String> = patterns.iter().map(|path| (*path).to_owned()).collect();
    stage(site.repo.root(), &patterns, site.staging.root()).unwrap_or_else(|e| unreachable!("{e}"))
}

fn copied(staged: &Staged) -> Vec<String> {
    staged
        .copied
        .iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn skipped(staged: &Staged) -> Vec<(String, Skip)> {
    staged
        .skipped
        .iter()
        .map(|(path, skip)| (path.display().to_string(), *skip))
        .collect()
}

fn entries(root: &Path) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap_or_else(|e| unreachable!("{e}")) {
            let entry = entry.unwrap_or_else(|e| unreachable!("{e}"));
            let kind = entry.file_type().unwrap_or_else(|e| unreachable!("{e}"));
            let relative = entry
                .path()
                .strip_prefix(root)
                .unwrap_or_else(|e| unreachable!("{e}"))
                .display()
                .to_string();
            found.push((relative, kind.is_symlink()));
            if kind.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

fn no_links_in_staging(site: &Site) {
    let links: Vec<_> = entries(site.staging.root())
        .into_iter()
        .filter(|(_, is_link)| *is_link)
        .collect();
    assert!(links.is_empty(), "{links:?}");
}

fn read(site: &Site, path: &str) -> String {
    fs::read_to_string(site.staging.root().join(path)).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_link_to_a_file_outside_the_workspace_inside_an_artifact_directory_is_not_staged() {
    let site = site("stage-file-link");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("logs/leak.log"),
    );
    let result = staged(&site, &["logs/"]);
    assert_eq!(copied(&result), ["logs/real.log"]);
    assert_eq!(skipped(&result), [("logs/leak.log".to_owned(), Skip::Link)]);
    assert_eq!(read(&site, "logs/real.log"), "ok\n");
    no_links_in_staging(&site);
    assert!(site.outside.root().join("credentials").is_file());
    assert!(
        fs::symlink_metadata(site.repo.root().join("logs/leak.log")).is_ok(),
        "the workspace is not modified"
    );
}

#[test]
fn a_declared_path_that_is_a_link_to_an_outside_directory_stages_nothing() {
    let site = site("stage-directory-link");
    link(
        &site.outside.root().join("dir"),
        &site.repo.root().join("logs"),
    );
    let result = staged(&site, &["logs/"]);
    assert_eq!(copied(&result), Vec::<String>::new());
    assert_eq!(skipped(&result), [("logs".to_owned(), Skip::Link)]);
    assert_eq!(entries(site.staging.root()), Vec::<(String, bool)>::new());
}

#[test]
fn a_link_in_the_middle_of_a_glob_is_not_followed() {
    let site = site("stage-middle-link");
    link(
        &site.outside.root().join("dir"),
        &site.repo.root().join("reports/escape"),
    );
    site.repo.write_unstaged("reports/real/a.log", "ok\n");
    let result = staged(&site, &["reports/*/a.log", "reports/**/*.log"]);
    assert_eq!(copied(&result), ["reports/real/a.log"]);
    assert_eq!(skipped(&result), Vec::<(String, Skip)>::new());
    assert!(!site.staging.root().join("reports/escape").exists());
    no_links_in_staging(&site);
}

#[test]
fn a_link_to_a_file_inside_the_workspace_is_not_staged_either() {
    let site = site("stage-inside-link");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    link(
        &site.repo.root().join("logs/real.log"),
        &site.repo.root().join("logs/alias.log"),
    );
    let result = staged(&site, &["logs/*.log"]);
    assert_eq!(copied(&result), ["logs/real.log"]);
    assert_eq!(
        skipped(&result),
        [("logs/alias.log".to_owned(), Skip::Link)]
    );
}

#[test]
fn only_the_selected_files_are_staged_with_their_relative_paths() {
    let site = site("stage-select");
    site.repo.write_unstaged("junit.xml", "<x/>\n");
    site.repo.write_unstaged("logs/a/one.log", "1\n");
    site.repo.write_unstaged("logs/a/one.txt", "no\n");
    site.repo.write_unstaged("snapshots/s/x.snap", "snap\n");
    site.repo.write_unstaged("other/leave.xml", "no\n");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("other/leak"),
    );
    let result = staged(
        &site,
        &[
            "junit.xml",
            "logs/**/*.log",
            "snapshots/",
            "missing/**/*.log",
        ],
    );
    assert_eq!(
        copied(&result),
        ["junit.xml", "logs/a/one.log", "snapshots/s/x.snap"]
    );
    assert_eq!(skipped(&result), Vec::<(String, Skip)>::new());
    assert_eq!(read(&site, "snapshots/s/x.snap"), "snap\n");
    assert!(!site.staging.root().join("other").exists());
}

#[test]
fn staging_empties_the_directory_first_and_replaces_a_link_in_its_place() {
    let site = site("stage-fresh");
    site.staging.write_unstaged("old/stale.log", "stale\n");
    site.repo.write_unstaged("junit.xml", "<x/>\n");
    staged(&site, &["junit.xml"]);
    assert_eq!(
        entries(site.staging.root()),
        [("junit.xml".to_owned(), false)]
    );
    let at = site.staging.root().to_path_buf();
    fs::remove_dir_all(&at).unwrap_or_else(|e| unreachable!("{e}"));
    link(site.outside.root(), &at);
    staged(&site, &["junit.xml"]);
    assert!(
        fs::symlink_metadata(&at)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .is_dir()
    );
    assert!(site.outside.root().join("credentials").is_file());
    assert!(!site.outside.root().join("junit.xml").exists());
}

#[test]
fn a_link_made_after_staging_cannot_change_the_staged_copy() {
    let site = site("stage-race");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    site.repo.write_unstaged("junit.xml", "<x/>\n");
    let result = staged(&site, &["logs/", "junit.xml"]);
    assert_eq!(copied(&result), ["junit.xml", "logs/real.log"]);
    fs::remove_file(site.repo.root().join("logs/real.log")).unwrap_or_else(|e| unreachable!("{e}"));
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("logs/real.log"),
    );
    fs::remove_dir_all(site.repo.root().join("logs")).unwrap_or_else(|e| unreachable!("{e}"));
    link(
        &site.outside.root().join("dir"),
        &site.repo.root().join("logs"),
    );
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("late.log"),
    );
    assert_eq!(read(&site, "logs/real.log"), "ok\n");
    assert_eq!(read(&site, "junit.xml"), "<x/>\n");
    assert_eq!(
        entries(site.staging.root()),
        [
            ("junit.xml".to_owned(), false),
            ("logs".to_owned(), false),
            ("logs/real.log".to_owned(), false),
        ]
    );
    no_links_in_staging(&site);
}

#[test]
fn a_staged_file_is_a_copy_so_a_later_edit_or_a_hard_link_target_change_does_not_reach_it() {
    let site = site("stage-copy");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    fs::hard_link(
        site.repo.root().join("logs/real.log"),
        site.repo.root().join("logs/hard.log"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let result = staged(&site, &["logs/"]);
    assert_eq!(copied(&result), ["logs/hard.log", "logs/real.log"]);
    site.repo.write_unstaged("logs/real.log", "changed\n");
    assert_eq!(read(&site, "logs/real.log"), "ok\n");
    assert_eq!(read(&site, "logs/hard.log"), "ok\n");
}

#[test]
fn a_pipe_is_skipped_and_never_opened() {
    let site = site("stage-pipe");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    nix::unistd::mkfifo(
        &site.repo.root().join("logs/pipe.log"),
        nix::sys::stat::Mode::S_IRWXU,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let result = staged(&site, &["logs/"]);
    assert_eq!(copied(&result), ["logs/real.log"]);
    assert_eq!(
        skipped(&result),
        [("logs/pipe.log".to_owned(), Skip::Special)]
    );
}

#[test]
fn a_file_under_a_secret_name_matched_by_a_broad_glob_is_skipped() {
    let site = site("stage-secret");
    site.repo.write_unstaged("app/out/report.xml", "<x/>\n");
    site.repo.write_unstaged("app/secrets/db.xml", "pw\n");
    site.repo.write_unstaged("app/out/api_token.xml", "pw\n");
    site.repo.write_unstaged("app/out/.env", "pw\n");
    site.repo.write_unstaged("app/.git/config", "pw\n");
    site.repo.write_unstaged("app/certs/server.pem", "pw\n");
    let result = staged(&site, &["app/**", "app/*/*.xml", "app/*/*"]);
    assert_eq!(copied(&result), ["app/out/report.xml"]);
    let skipped = skipped(&result);
    for secret in [
        "app/secrets",
        "app/out/api_token.xml",
        "app/certs/server.pem",
    ] {
        assert!(
            skipped.contains(&(secret.to_owned(), Skip::Secret)),
            "{secret}: {skipped:?}"
        );
    }
    assert!(skipped.iter().all(|(_, skip)| *skip == Skip::Secret));
    assert!(!site.staging.root().join("app/secrets").exists());
}

#[test]
fn the_command_warns_once_per_skipped_entry_without_forging_a_workflow_command() {
    let site = site("stage-command");
    site.repo.write_unstaged("logs/real.log", "ok\n");
    site.repo.write_unstaged("logs/secrets/a.log", "pw\n");
    link(
        &site.outside.root().join("credentials"),
        &site.repo.root().join("logs/x\n::error::FORGED"),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(site.outside.root())
        .env("GITHUB_WORKSPACE", site.repo.root())
        .args(["artifacts", "stage", "--into"])
        .arg(site.staging.root())
        .args(["--path", "logs/", "--path", "junit.xml"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().filter(|line| line.starts_with("::")).count(),
        2,
        "{stdout}"
    );
    assert!(
        stdout.contains("::warning::") && stdout.contains("%0A::error::FORGED"),
        "{stdout}"
    );
    assert!(stdout.contains("logs/secrets"), "{stdout}");
    assert_eq!(read(&site, "logs/real.log"), "ok\n");
    no_links_in_staging(&site);
}
