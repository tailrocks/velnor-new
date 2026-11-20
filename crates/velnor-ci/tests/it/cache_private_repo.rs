// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::fetch::{Api, TestResult, serve, serve_body};
use crate::io::Repo as Checkout;

const COMPILED: &str =
    "   Compiling app v0.1.0 (/work/lab/app)\n    Finished `dev` profile in 3.20s\n";
pub(crate) const TOKEN: &str = "ghs_job_token";
pub(crate) const PRIVATE: &str =
    r#"{"full_name":"acme/app","private":true,"visibility":"private"}"#;
const PUBLIC: &str = r#"{"full_name":"acme/app","private":false,"visibility":"public"}"#;
const INTERNAL: &str = r#"{"full_name":"acme/app","private":true,"visibility":"internal"}"#;
const OTHER: &str = r#"{"full_name":"acme/other","private":true,"visibility":"private"}"#;
pub(crate) const GIT_LOCK: &str = "version = 4\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?rev=abc#0123456789abcdef0123456789abcdef01234567\"\n";

struct Outcome {
    output: Output,
    written: String,
    api: Api,
}

impl Outcome {
    fn requests(&self) -> Vec<String> {
        self.api
            .requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }
}

fn verdict(name: &str, status: u16, body: &str, token: Option<&str>) -> TestResult2 {
    let repo = Checkout::empty(name);
    let outputs = repo.root().join("github-output");
    let api = serve_body(status, body)?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .current_dir(repo.root())
        .args(["cache", "private-repo"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_REPOSITORY", "acme/app")
        .env("VELNOR_API_URL", &api.url);
    if let Some(token) = token {
        command.env("GITHUB_TOKEN", token);
    }
    let output = command.output()?;
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    Ok(Outcome {
        output,
        written,
        api,
    })
}

type TestResult2 = Result<Outcome, Box<dyn std::error::Error>>;

#[test]
fn a_private_repository_is_reported_private_with_the_job_token() -> TestResult {
    let outcome = verdict("pr-private", 200, PRIVATE, Some(TOKEN))?;
    assert!(outcome.output.status.success(), "{:?}", outcome.output);
    assert_eq!(outcome.written, "private=true\n");
    assert_eq!(
        outcome.requests(),
        [format!(
            "get /repos/acme/app http/1.1 | authorization: bearer {TOKEN}"
        )]
    );
    Ok(())
}

fn not_private(cases: &[(&str, u16, &str, Option<&str>)]) -> TestResult {
    for (label, status, body, token) in cases {
        let outcome = verdict(&format!("pr-{label}"), *status, body, *token)?;
        assert!(
            outcome.output.status.success(),
            "{label}: {:?}",
            outcome.output
        );
        assert_eq!(outcome.written, "private=false\n", "{label}");
        if token.is_none() {
            assert!(outcome.requests().is_empty(), "{label}");
        }
    }
    Ok(())
}

#[test]
fn a_public_or_internal_repository_is_reported_not_private() -> TestResult {
    not_private(&[
        ("public", 200, PUBLIC, Some(TOKEN)),
        ("internal", 200, INTERNAL, Some(TOKEN)),
    ])
}

#[test]
fn a_failed_lookup_is_reported_not_private_and_still_exits_zero() -> TestResult {
    not_private(&[
        ("not-found", 404, r#"{"message":"Not Found"}"#, Some(TOKEN)),
        ("malformed", 200, "not json", Some(TOKEN)),
    ])
}

#[test]
fn an_answer_that_does_not_prove_privacy_of_this_repository_is_reported_not_private() -> TestResult
{
    not_private(&[
        ("another-repository", 200, OTHER, Some(TOKEN)),
        (
            "no-private-field",
            200,
            r#"{"full_name":"acme/app"}"#,
            Some(TOKEN),
        ),
        (
            "no-visibility",
            200,
            r#"{"full_name":"acme/app","private":true}"#,
            Some(TOKEN),
        ),
        (
            "no-full-name",
            200,
            r#"{"private":true,"visibility":"private"}"#,
            Some(TOKEN),
        ),
        ("no-token", 200, PRIVATE, None),
    ])
}

#[test]
fn the_job_token_goes_only_to_an_https_api_or_a_loopback_host() -> TestResult {
    for api in [
        "http://127.0.0.1.evil.example",
        "http://127.0.0.1@evil.example",
        "http://localhost.evil.example",
        "http://evil.example",
        "ftp://127.0.0.1",
    ] {
        let repo = Checkout::empty("pr-untrusted-api");
        let outputs = repo.root().join("github-output");
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .current_dir(repo.root())
            .args(["cache", "private-repo"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GITHUB_OUTPUT", &outputs)
            .env("GITHUB_REPOSITORY", "acme/app")
            .env("GITHUB_TOKEN", TOKEN)
            .env("VELNOR_API_URL", api)
            .output()?;
        assert!(output.status.success(), "{api}: {output:?}");
        assert_eq!(fs::read_to_string(&outputs)?, "private=false\n", "{api}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("set VELNOR_API_URL"),
            "{api}: {output:?}"
        );
    }
    Ok(())
}

pub(crate) fn decide_with(
    repo: &Checkout,
    api: Option<&Api>,
    token: Option<&str>,
) -> (Output, String) {
    decide_in(repo, api, token, None)
}

pub(crate) fn decide_in(
    repo: &Checkout,
    api: Option<&Api>,
    token: Option<&str>,
    cargo_home: Option<&str>,
) -> (Output, String) {
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), COMPILED).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = repo.root().join("github-output");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["cache", "decide", "--target-dir", "target"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", repo.root().join("home"))
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_REPOSITORY", "acme/app")
        .current_dir(repo.root());
    if let Some(api) = api {
        command.env("VELNOR_API_URL", &api.url);
    }
    if let Some(token) = token {
        command.env("GITHUB_TOKEN", token);
    }
    if let Some(cargo_home) = cargo_home {
        command.env("CARGO_HOME", repo.root().join(cargo_home));
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&outputs).unwrap_or_default())
}

pub(crate) fn private_checkout(name: &str, lock: &str) -> Checkout {
    let repo = Checkout::new(name);
    repo.write_unstaged("target/debug/incremental/app-1/work", "x");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("Cargo.lock", lock);
    fs::create_dir_all(repo.root().join("home")).unwrap_or_else(|e| unreachable!("{e}"));
    repo
}

fn refused(repo: &Checkout, output: &Output, written: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("not proven public"),
        "{output:?}"
    );
    assert!(repo.root().join("target/debug/incremental").is_dir());
}

#[test]
fn a_private_repository_saves_the_base_caches_for_private_git_sources() -> TestResult {
    let repo = private_checkout("decide-private-repo", GIT_LOCK);
    let api = serve_body(200, PRIVATE)?;
    let (output, written) = decide_with(&repo, Some(&api), Some(TOKEN));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    assert!(!repo.root().join("target/debug/incremental").exists());
    let requests = api.requests.lock().map_err(|e| e.to_string())?.clone();
    assert_eq!(
        requests,
        [format!(
            "get /repos/acme/app http/1.1 | authorization: bearer {TOKEN}"
        )]
    );
    Ok(())
}

fn refuses_git_sources(cases: &[(&str, u16, &str, Option<&str>)]) -> TestResult {
    for (label, status, body, token) in cases {
        let repo = private_checkout(&format!("decide-refused-{label}"), GIT_LOCK);
        let api = serve_body(*status, body)?;
        let (output, written) = decide_with(&repo, Some(&api), *token);
        refused(&repo, &output, &written);
    }
    Ok(())
}

#[test]
fn a_public_or_internal_repository_keeps_refusing_private_git_sources() -> TestResult {
    refuses_git_sources(&[
        ("public", 200, PUBLIC, Some(TOKEN)),
        ("internal", 200, INTERNAL, Some(TOKEN)),
    ])
}

#[test]
fn an_unverifiable_repository_keeps_refusing_private_git_sources() -> TestResult {
    refuses_git_sources(&[
        ("api-error", 500, "oops", Some(TOKEN)),
        ("another-repository", 200, OTHER, Some(TOKEN)),
    ])
}

#[test]
fn no_token_or_no_api_refuses_private_git_sources() -> TestResult {
    refuses_git_sources(&[("no-token", 200, PRIVATE, None)])?;
    let repo = private_checkout("decide-no-api", GIT_LOCK);
    let (output, written) = decide_with(&repo, None, Some(TOKEN));
    refused(&repo, &output, &written);
    Ok(())
}

fn refuses_lock(cases: &[(&str, &str)]) -> TestResult {
    for (label, lock) in cases {
        let repo = private_checkout(&format!("decide-private-other-{label}"), lock);
        let api = serve_body(200, PRIVATE)?;
        let (output, written) = decide_with(&repo, Some(&api), Some(TOKEN));
        refused(&repo, &output, &written);
    }
    Ok(())
}

#[test]
fn a_private_repository_still_refuses_a_registry_or_a_patch() -> TestResult {
    refuses_lock(&[
        (
            "registry",
            "version = 4\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"registry+https://private.example/index\"\n",
        ),
        (
            "patch",
            &format!(
                "{GIT_LOCK}[[patch.unused]]\nname = \"secret\"\nversion = \"1.0.0\"\nsource = \"git+https://example.com/private#123\"\n"
            ),
        ),
    ])
}

#[test]
fn a_private_repository_still_refuses_a_git_url_with_credentials() -> TestResult {
    refuses_lock(&[
        (
            "credentials-in-url",
            "version = 4\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"git+https://user:secret@github.com/acme/shared-lib#123\"\n",
        ),
        (
            "token-in-url",
            "version = 4\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"git+https://ghp_abc@github.com/acme/shared-lib#123\"\n",
        ),
    ])
}

#[test]
fn a_cargo_config_with_registries_or_sources_is_refused_even_in_a_private_repository() -> TestResult
{
    for config in [
        "[registries.private]\nindex = \"sparse+https://registry.example.invalid/\"\n",
        "[source.mirror]\nregistry = \"https://registry.example.invalid/\"\n",
    ] {
        let repo = private_checkout("decide-private-config", GIT_LOCK);
        repo.write_unstaged(".cargo/config.toml", config);
        let api = serve_body(200, PRIVATE)?;
        let (output, written) = decide_with(&repo, Some(&api), Some(TOKEN));
        refused(&repo, &output, &written);
    }
    Ok(())
}

#[test]
fn a_public_lock_asks_no_question_of_the_api() -> TestResult {
    let repo = private_checkout("decide-public-lock-no-api", "version = 4\n");
    let api = serve(500)?;
    let (output, written) = decide_with(&repo, Some(&api), Some(TOKEN));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert!(api.requests.lock().map_err(|e| e.to_string())?.is_empty());
    Ok(())
}
