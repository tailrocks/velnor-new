// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::net::TcpListener;
use std::process::{Command, Output};

use super::public_git::{
    OTHER_SHA, SHA, TOKEN, decide_with, git, lock, public, refused, reply, serve,
};
use super::support::seed;
use crate::io::Repo as Checkout;

const VERDICTS: &str = "VELNOR_PUBLIC_VERDICTS";

fn public_sources(sources: &[(&str, String)], api: &str, env: &[(&str, &str)]) -> (Output, String) {
    let repo = Checkout::new("public-sources");
    seed(&repo);
    repo.write_unstaged("Cargo.lock", &lock(sources));
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "public-sources", "--public-probe-api", api])
        .env("GITHUB_OUTPUT", &outputs)
        .env_remove("GITHUB_TOKEN")
        .envs(env.iter().copied())
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written)
}

fn closed_api() -> String {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let url = format!(
        "http://{}",
        closed.local_addr().unwrap_or_else(|e| unreachable!("{e}"))
    );
    drop(closed);
    url
}

fn everything(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn the_run_wide_step_looks_each_repository_up_once_and_writes_the_verdicts() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [
        ("one", git("acme/shared-lib", SHA)),
        ("two", git("acme/shared-lib", OTHER_SHA)),
    ];
    let (output, written) = public_sources(&sources, &mock.url, &[("GITHUB_TOKEN", TOKEN)]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "verdicts={\"acme/shared-lib\":true}\n");
    assert_eq!(mock.requests().len(), 1, "{:?}", mock.requests());
    assert!(!everything(&output).contains(TOKEN));
}

#[test]
fn the_run_wide_step_records_a_failed_lookup_and_still_succeeds() {
    let mock = serve(reply(404, String::new()));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = public_sources(&sources, &mock.url, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "verdicts={\"acme/shared-lib\":false}\n");
    let (output, written) = public_sources(&sources, &closed_api(), &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "verdicts={\"acme/shared-lib\":false}\n");
}

#[test]
fn the_run_wide_step_writes_an_empty_map_when_no_lock_names_a_git_repository() {
    let mock = serve(public("acme/shared-lib"));
    let (output, written) = public_sources(&[], &mock.url, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "verdicts={}\n");
    assert!(mock.requests().is_empty());
}

#[test]
fn the_run_wide_step_does_not_look_up_a_source_decide_would_refuse() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", format!("git+ssh://git@github.com/acme/shared-lib#{SHA}"))];
    let (output, written) = public_sources(&sources, &mock.url, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "verdicts={}\n");
    assert!(mock.requests().is_empty());
}

fn with_verdicts<'a>(text: &'a str, token: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut env = vec![(VERDICTS, text)];
    env.extend_from_slice(token);
    env
}

#[test]
fn a_public_verdict_of_this_run_replaces_the_lookup() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let env = with_verdicts(r#"{"acme/shared-lib":true}"#, &[("GITHUB_TOKEN", TOKEN)]);
    let (output, written) = decide_with(&sources, &mock.url, &env);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{stdout}");
    assert!(mock.requests().is_empty(), "{:?}", mock.requests());
    assert!(
        stdout.contains("git dependency github.com/acme/shared-lib is public (verdict of this run)"),
        "{stdout}"
    );
}

fn probes_for_itself(verdicts: &str) {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide_with(&sources, &mock.url, &with_verdicts(verdicts, &[]));
    assert!(output.status.success(), "{verdicts}: {output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{verdicts}");
    assert_eq!(mock.requests().len(), 1, "{verdicts}: {:?}", mock.requests());
}

macro_rules! probes {
    ($name:ident, $verdicts:expr) => {
        #[test]
        fn $name() {
            probes_for_itself($verdicts);
        }
    };
}

probes!(a_private_verdict_is_probed_again, r#"{"acme/shared-lib":false}"#);
probes!(a_verdict_for_another_repository_is_ignored, r#"{"acme/other":true}"#);
probes!(an_empty_verdict_is_ignored, "{}");
probes!(an_unreadable_verdict_is_ignored, "not json");
probes!(a_verdict_that_is_not_a_map_is_ignored, "[true]");
probes!(a_verdict_with_a_text_value_is_ignored, r#"{"acme/shared-lib":"true"}"#);
probes!(a_blank_verdict_is_ignored, "");

#[test]
fn a_verdict_covers_only_the_repositories_it_names() {
    let mock = serve(public("acme/other-lib"));
    let sources = [
        ("one", git("acme/shared-lib", SHA)),
        ("two", git("acme/other-lib", SHA)),
    ];
    let env = with_verdicts(r#"{"acme/shared-lib":true}"#, &[]);
    let (_, written) = decide_with(&sources, &mock.url, &env);
    assert_eq!(written, "save=true\nsources=true\n");
    let requests = mock.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].starts_with("get /repos/acme/other-lib "), "{requests:?}");
}

#[test]
fn a_public_verdict_does_not_admit_a_source_the_lock_check_refuses() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [
        ("dep", git("acme/shared-lib", SHA)),
        ("secret", "git+ssh://git@github.com/acme/private#abc".to_owned()),
    ];
    let env = with_verdicts(r#"{"acme/shared-lib":true}"#, &[]);
    let (output, written) = decide_with(&sources, &mock.url, &env);
    refused(&output, &written);
    assert!(mock.requests().is_empty());
}

#[test]
fn a_failed_lookup_never_prints_the_token() {
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let env = [("GITHUB_TOKEN", TOKEN), ("GH_TOKEN", TOKEN)];
    for api in [closed_api(), serve(reply(403, TOKEN.to_owned())).url] {
        let (output, written) = decide_with(&sources, &api, &env);
        refused(&output, &written);
        assert!(!everything(&output).contains(TOKEN), "{output:?}");
        assert!(!written.contains(TOKEN));
    }
}
