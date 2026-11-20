// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use super::public_git::{SHA, decide, decide_declaring, git, lock, public, refused, serve};
use super::support::seed;
use crate::io::Repo as Checkout;

pub(super) fn declare(repo: &Checkout, repositories: &[String]) {
    if repositories.is_empty() {
        return;
    }
    let listed: Vec<String> = repositories
        .iter()
        .map(|name| format!("\"https://github.com/{name}\""))
        .collect();
    repo.write_unstaged(
        ".velnor/rust-ci.toml",
        &format!("[public_sources]\ngit = [{}]\n", listed.join(", ")),
    );
}

pub(super) fn repositories(sources: &[(&str, String)]) -> Vec<String> {
    let mut names: Vec<String> = sources
        .iter()
        .filter_map(|(_, source)| source.strip_prefix("git+https://github.com/"))
        .filter_map(|rest| rest.split(['?', '#']).next())
        .map(|name| name.strip_suffix(".git").unwrap_or(name).to_owned())
        .collect();
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

fn undeclared_notice(stdout: &str, name: &str) {
    assert!(
        stdout.contains(&format!(
            "git dependency github.com/{name} is not declared in [public_sources]"
        )),
        "{stdout}"
    );
}

#[test]
fn a_locked_repository_that_is_not_declared_is_not_cacheable_and_is_named() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [
        ("dep", git("acme/shared-lib", SHA)),
        ("extra", git("acme/extra-lib", SHA)),
    ];
    let declared = ["acme/shared-lib".to_owned()];
    let (output, written) = decide_declaring(&sources, &declared, &mock.url, &[]);
    let stdout = refused(&output, &written);
    undeclared_notice(&stdout, "acme/extra-lib");
    assert!(!stdout.contains("acme/shared-lib"), "{stdout}");
}

#[test]
fn the_probe_never_runs_for_an_undeclared_repository() {
    let mock = serve(public("acme/extra-lib"));
    let sources = [("extra", git("acme/extra-lib", SHA))];
    let declared = ["acme/other-lib".to_owned()];
    let (output, written) = decide_declaring(&sources, &declared, &mock.url, &[]);
    undeclared_notice(&refused(&output, &written), "acme/extra-lib");
    assert_eq!(mock.requests(), Vec::<String>::new());
}

#[test]
fn a_git_dependency_without_any_declaration_is_not_cacheable() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide_declaring(&sources, &[], &mock.url, &[]);
    undeclared_notice(&refused(&output, &written), "acme/shared-lib");
    assert_eq!(mock.requests(), Vec::<String>::new());
}

#[test]
fn a_public_verdict_does_not_admit_an_undeclared_repository() {
    let mock = serve(public("acme/extra-lib"));
    let sources = [("extra", git("acme/extra-lib", SHA))];
    let declared = ["acme/other-lib".to_owned()];
    let (output, written) = decide_declaring(
        &sources,
        &declared,
        &mock.url,
        &[("VELNOR_PUBLIC_VERDICTS", r#"{"acme/extra-lib":true}"#)],
    );
    undeclared_notice(&refused(&output, &written), "acme/extra-lib");
}

#[test]
fn the_declaration_matches_case_insensitively() {
    let mock = serve(public("Acme/Shared-Lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let declared = ["ACME/SHARED-LIB".to_owned()];
    let (output, written) = decide_declaring(&sources, &declared, &mock.url, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
}

#[test]
fn a_declared_repository_alone_is_cacheable() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    assert_eq!(repositories(&sources), ["acme/shared-lib"]);
    let (output, written) = decide(&sources, &mock.url);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
}

#[test]
fn an_unreadable_declaration_is_not_cacheable() {
    let mock = serve(public("acme/shared-lib"));
    let repo = Checkout::new("decide-public-bad-config");
    seed(&repo);
    repo.write_unstaged("Cargo.lock", &lock(&[("dep", git("acme/shared-lib", SHA))]));
    repo.write_unstaged(".velnor/rust-ci.toml", "[public_sources\n");
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), super::support::COMPILED)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = repo.root().join("github-output");
    let output: Output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "decide", "--target-dir", "target"])
        .args(["--public-probe-api", &mock.url])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env_remove("GITHUB_TOKEN")
        .env_remove("VELNOR_PUBLIC_VERDICTS")
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n");
    assert_eq!(mock.requests(), Vec::<String>::new());
}
