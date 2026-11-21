// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use super::public_declared::{declare, repositories};
use super::public_git::{
    OTHER_SHA, Reply, SHA, decide, git, lock, public, refused, reply, serve, serve_tree,
};
use super::support::{COMPILED, seed};
use crate::io::Repo as Checkout;

const PRIVATE: &str = r#"{"full_name":"acme/shared-lib","private":true}"#;

fn with_gitmodules() -> Reply {
    reply(
        200,
        r#"{"sha":"0","tree":[{"path":".gitmodules","type":"blob"},{"path":"vendor","type":"commit"}],"truncated":false}"#
            .to_owned(),
    )
}

fn run(locks: &[(&str, String)], api: &str, command: &[&str], extra: &[&str]) -> (Output, String) {
    let repo = Checkout::new("public-guards");
    seed(&repo);
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), COMPILED).unwrap_or_else(|e| unreachable!("{e}"));
    let mut args: Vec<String> = Vec::new();
    let mut declared: Vec<String> = Vec::new();
    for (directory, text) in locks {
        repo.write_unstaged(&format!("{directory}Cargo.lock"), text);
        declared.extend(text.lines().filter_map(|line| {
            let source = line.strip_prefix("source = \"")?.strip_suffix('"')?;
            repositories(&[("", source.to_owned())]).pop()
        }));
        args.extend([
            "--manifest-path".to_owned(),
            format!("{directory}Cargo.toml"),
        ]);
    }
    declared.sort_by_key(|name| name.to_ascii_lowercase());
    declared.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    declare(&repo, &declared);
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("cache")
        .args(command)
        .args(["--public-probe-api", api])
        .args(&args)
        .args(extra)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env_remove("GITHUB_TOKEN")
        .env_remove("VELNOR_PUBLIC_VERDICTS")
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written)
}

fn decide_locks(locks: &[(&str, String)], api: &str, extra: &[&str]) -> (Output, String) {
    run(locks, api, &["decide", "--target-dir", "target"], extra)
}

fn one(sources: &[(&str, String)]) -> Vec<(&'static str, String)> {
    vec![("", lock(sources))]
}

fn fails_the_run(reply: Reply) {
    let mock = serve(reply);
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{stderr}");
    assert!(stderr.contains("github.com/acme/shared-lib"), "{stderr}");
    assert!(stderr.contains("[private_sources]"), "{stderr}");
}

#[test]
fn a_declared_public_repository_that_is_private_fails_the_run() {
    fails_the_run(reply(200, PRIVATE.to_owned()));
}

#[test]
fn a_declared_public_repository_that_answers_not_found_fails_the_run() {
    fails_the_run(reply(404, r#"{"message":"Not Found"}"#.to_owned()));
}

#[test]
fn an_inconclusive_lookup_keeps_the_run_green() {
    for status in [403, 429, 500, 502] {
        let mock = serve(reply(status, String::new()));
        let sources = [("dep", git("acme/shared-lib", SHA))];
        let (output, written) = decide(&sources, &mock.url);
        refused(&output, &written);
    }
}

#[test]
fn a_public_repository_with_submodules_is_not_cacheable() {
    let mock = serve_tree(public("acme/shared-lib"), with_gitmodules());
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    let stdout = refused(&output, &written);
    assert!(stdout.contains("submodules"), "{stdout}");
    assert!(stdout.contains("github.com/acme/shared-lib"), "{stdout}");
    let trees = mock.trees();
    assert_eq!(trees.len(), 1, "{trees:?}");
    assert!(
        trees[0].starts_with(&format!("get /repos/acme/shared-lib/git/trees/{SHA} ")),
        "{}",
        trees[0]
    );
    assert!(!trees[0].contains("authorization"), "{}", trees[0]);
}

#[test]
fn every_locked_commit_of_a_repository_is_checked_for_submodules() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [
        ("one", git("acme/shared-lib", SHA)),
        ("two", git("acme/shared-lib", OTHER_SHA)),
    ];
    let (_, written) = decide(&sources, &mock.url);
    assert_eq!(written, "save=true\nsources=true\n");
    assert_eq!(mock.requests().len(), 1);
    let trees = mock.trees();
    assert_eq!(trees.len(), 2, "{trees:?}");
    assert!(trees.iter().any(|tree| tree.contains(SHA)), "{trees:?}");
    assert!(
        trees.iter().any(|tree| tree.contains(OTHER_SHA)),
        "{trees:?}"
    );
}

#[test]
fn a_file_tree_that_cannot_be_read_is_not_cacheable() {
    let broken = [
        reply(500, String::new()),
        reply(404, r#"{"message":"Not Found"}"#.to_owned()),
        reply(200, "not json".to_owned()),
        reply(200, r#"{"sha":"0","tree":[],"truncated":true}"#.to_owned()),
    ];
    for tree in broken {
        let mock = serve_tree(public("acme/shared-lib"), tree);
        let sources = [("dep", git("acme/shared-lib", SHA))];
        let (output, written) = decide(&sources, &mock.url);
        refused(&output, &written);
    }
}

#[test]
fn a_case_variant_gitmodules_is_still_a_submodule_declaration() {
    for name in [".GitModules", ".GITMODULES"] {
        let tree = reply(
            200,
            format!(
                r#"{{"sha":"0","tree":[{{"path":"{name}","type":"blob"}}],"truncated":false}}"#
            ),
        );
        let mock = serve_tree(public("acme/shared-lib"), tree);
        let sources = [("dep", git("acme/shared-lib", SHA))];
        let (output, written) = decide(&sources, &mock.url);
        let stdout = refused(&output, &written);
        assert!(stdout.contains("submodules"), "{name}: {stdout}");
    }
}

#[test]
fn a_gitlink_entry_is_a_submodule_even_without_a_gitmodules_file() {
    let tree = reply(
        200,
        r#"{"sha":"0","tree":[{"path":"vendor","type":"commit","mode":"160000"}],"truncated":false}"#
            .to_owned(),
    );
    let mock = serve_tree(public("acme/shared-lib"), tree);
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    let stdout = refused(&output, &written);
    assert!(stdout.contains("submodules"), "{stdout}");
}

#[test]
fn the_run_wide_lookup_records_a_repository_with_submodules_as_not_public() {
    let mock = serve_tree(public("acme/shared-lib"), with_gitmodules());
    let locks = one(&[("dep", git("acme/shared-lib", SHA))]);
    let (output, written) = run(&locks, &mock.url, &["public-sources"], &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "verdicts={\"acme/shared-lib\":false}\n");
}

#[test]
fn the_probes_share_one_time_budget() {
    let mut slow = public("acme/shared-lib");
    slow.delay = Duration::from_secs(10);
    let mock = serve(slow);
    let names = ["one", "two", "three", "four", "five", "six"];
    let sources: Vec<(&str, String)> = names
        .iter()
        .map(|name| (*name, git(&format!("acme/lib-{name}"), SHA)))
        .collect();
    let started = Instant::now();
    let (output, written) =
        decide_locks(&one(&sources), &mock.url, &["--public-probe-seconds", "1"]);
    let elapsed = started.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{stdout}");
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
    assert!(stdout.contains("time budget"), "{stdout}");
    assert_eq!(
        stdout.matches("is not proven public").count(),
        6,
        "{stdout}"
    );
}

#[test]
fn a_failed_lookup_is_reported_once() {
    let mock = serve(reply(403, String::new()));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    let stdout = refused(&output, &written);
    assert_eq!(stdout.matches("::notice::").count(), 1, "{stdout}");
    assert_eq!(
        stdout.matches("is not proven public").count(),
        1,
        "{stdout}"
    );
}

#[test]
fn two_spellings_of_one_repository_in_different_locks_are_looked_up_once() {
    let mock = serve(public("acme/shared-lib"));
    let locks = vec![
        ("a/", lock(&[("dep", git("acme/shared-lib", SHA))])),
        ("b/", lock(&[("dep", git("ACME/Shared-Lib", SHA))])),
    ];
    let (output, written) = decide_locks(&locks, &mock.url, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert_eq!(mock.requests().len(), 1, "{:?}", mock.requests());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("is public").count(), 1, "{stdout}");
}
