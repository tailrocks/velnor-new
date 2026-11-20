// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use super::support::{COMPILED, seed};
use crate::io::Repo as Checkout;

pub(super) const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
pub(super) const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";
pub(super) const TOKEN: &str = "ghs_job_token_must_not_leave";

pub(super) struct Reply {
    pub(super) status: u16,
    pub(super) headers: Vec<(&'static str, String)>,
    pub(super) body: String,
}

pub(super) fn public(name: &str) -> Reply {
    reply(200, format!(r#"{{"full_name":"{name}","private":false}}"#))
}

pub(super) fn reply(status: u16, body: String) -> Reply {
    Reply {
        status,
        headers: Vec::new(),
        body,
    }
}

pub(super) struct Mock {
    pub(super) url: String,
    pub(super) requests: Arc<Mutex<Vec<String>>>,
}

pub(super) fn serve(reply: Reply) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let url = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            if let Ok(mut seen) = seen.lock() {
                seen.push(String::from_utf8_lossy(&request).to_ascii_lowercase());
            }
            let mut lines = vec![format!("HTTP/1.1 {} Reply", reply.status)];
            lines.extend(reply.headers.iter().map(|(k, v)| format!("{k}: {v}")));
            lines.push(format!("content-length: {}", reply.body.len()));
            lines.push("connection: close".to_owned());
            let head = format!("{}\r\n\r\n", lines.join("\r\n"));
            stream.write_all(head.as_bytes()).ok();
            stream.write_all(reply.body.as_bytes()).ok();
        }
    });
    Mock { url, requests }
}

impl Mock {
    pub(super) fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .clone()
    }
}

pub(super) fn lock(sources: &[(&str, String)]) -> String {
    let packages: Vec<String> = sources
        .iter()
        .map(|(name, source)| {
            format!(
                "\n[[package]]\nname = \"{name}\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
            )
        })
        .collect();
    format!("version = 4\n{}", packages.concat())
}

pub(super) fn git(repository: &str, sha: &str) -> String {
    format!("git+https://github.com/{repository}?rev=v1#{sha}")
}

pub(super) fn decide(sources: &[(&str, String)], api: &str) -> (Output, String) {
    decide_with(
        sources,
        api,
        &[("GITHUB_TOKEN", TOKEN), ("GH_TOKEN", TOKEN)],
    )
}

pub(super) fn decide_with(
    sources: &[(&str, String)],
    api: &str,
    env: &[(&str, &str)],
) -> (Output, String) {
    let repo = Checkout::new("decide-public-git");
    seed(&repo);
    repo.write_unstaged("Cargo.lock", &lock(sources));
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), COMPILED).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "cache",
            "decide",
            "--target-dir",
            "target",
            "--public-probe-api",
            api,
        ])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env_remove("VELNOR_PUBLIC_VERDICTS")
        .envs(env.iter().copied())
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written)
}

pub(super) fn refused(output: &Output, written: &str) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{stdout}");
    assert!(stdout.contains("not proven public"), "{stdout}");
    stdout
}

#[test]
fn a_commit_pinned_github_dependency_of_a_public_repository_is_cacheable() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{stdout}");
    assert!(
        stdout.contains("git dependency github.com/acme/shared-lib is public"),
        "{stdout}"
    );
}

#[test]
fn the_lookup_sends_no_authorization_and_never_the_job_token() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    let requests = mock.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(
        requests[0].starts_with("get /repos/acme/shared-lib http/1.1\r\n"),
        "{}",
        requests[0]
    );
    assert!(!requests[0].contains("authorization"), "{}", requests[0]);
    assert!(
        !requests[0].contains(&TOKEN.to_ascii_lowercase()),
        "{}",
        requests[0]
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
}

#[test]
fn one_repository_is_looked_up_once_and_each_distinct_one_is_checked() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [
        ("one", git("acme/shared-lib", SHA)),
        ("two", git("acme/shared-lib", OTHER_SHA)),
    ];
    let (_, written) = decide(&sources, &mock.url);
    assert_eq!(written, "save=true\nsources=true\n");
    assert_eq!(mock.requests().len(), 1);
    let sources = [
        ("one", git("acme/shared-lib", SHA)),
        ("two", git("acme/other-lib", SHA)),
    ];
    let (output, written) = decide(&sources, &mock.url);
    let stdout = refused(&output, &written);
    assert!(stdout.contains("github.com/acme/other-lib"), "{stdout}");
    assert!(stdout.contains("is not proven public"), "{stdout}");
}

fn unreadable_reply_is_refused(reply: Reply) {
    let mock = serve(reply);
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    refused(&output, &written);
    assert_eq!(mock.requests().len(), 1);
}

macro_rules! refused_reply {
    ($name:ident, $reply:expr) => {
        #[test]
        fn $name() {
            unreadable_reply_is_refused($reply);
        }
    };
}

refused_reply!(
    a_private_repository_is_not_cacheable,
    reply(
        200,
        r#"{"full_name":"acme/shared-lib","private":true}"#.to_owned()
    )
);
refused_reply!(
    a_missing_or_private_to_anonymous_repository_is_not_cacheable,
    reply(404, r#"{"message":"Not Found"}"#.to_owned())
);
refused_reply!(
    a_rate_limit_is_not_cacheable,
    reply(403, r#"{"message":"API rate limit exceeded"}"#.to_owned())
);
refused_reply!(a_429_is_not_cacheable, reply(429, String::new()));
refused_reply!(a_server_error_is_not_cacheable, reply(500, String::new()));
refused_reply!(
    an_unreadable_body_is_not_cacheable,
    reply(200, "not json".to_owned())
);
refused_reply!(
    a_body_without_the_private_flag_is_not_cacheable,
    reply(200, r#"{"full_name":"acme/shared-lib"}"#.to_owned())
);
refused_reply!(
    a_private_flag_that_is_not_a_boolean_is_not_cacheable,
    reply(
        200,
        r#"{"full_name":"acme/shared-lib","private":"false"}"#.to_owned()
    )
);
refused_reply!(
    a_reply_for_another_repository_is_not_cacheable,
    public("acme/another-repository")
);

#[test]
fn a_redirect_to_another_repository_is_not_followed() {
    let target = serve(public("acme/shared-lib"));
    let mut moved = reply(301, String::new());
    moved
        .headers
        .push(("location", format!("{}/repos/acme/elsewhere", target.url)));
    let mock = serve(moved);
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &mock.url);
    refused(&output, &written);
    assert!(target.requests().is_empty(), "{:?}", target.requests());
}

#[test]
fn an_unreachable_api_is_not_cacheable() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let url = format!(
        "http://{}",
        closed.local_addr().unwrap_or_else(|e| unreachable!("{e}"))
    );
    drop(closed);
    let sources = [("dep", git("acme/shared-lib", SHA))];
    let (output, written) = decide(&sources, &url);
    refused(&output, &written);
}

fn refused_without_a_lookup(source: impl Into<String>) {
    let source = source.into();
    let mock = serve(public("acme/shared-lib"));
    let (output, written) = decide(&[("dep", source.clone())], &mock.url);
    refused(&output, &written);
    assert_eq!(mock.requests().len(), 0, "{source}: {:?}", mock.requests());
}

macro_rules! no_lookup {
    ($name:ident, $source:expr) => {
        #[test]
        fn $name() {
            refused_without_a_lookup($source);
        }
    };
}

no_lookup!(
    a_short_commit_is_refused_without_a_lookup,
    format!(
        "git+https://github.com/acme/shared-lib?rev=v1#{}",
        &SHA[..39]
    )
);
no_lookup!(
    an_upper_case_commit_is_refused_without_a_lookup,
    format!(
        "git+https://github.com/acme/shared-lib#{}",
        SHA.to_uppercase()
    )
);
no_lookup!(
    a_source_without_a_commit_is_refused_without_a_lookup,
    "git+https://github.com/acme/shared-lib?branch=main"
);
no_lookup!(
    ssh_is_refused_without_a_lookup,
    format!("git+ssh://git@github.com/acme/shared-lib#{SHA}")
);
no_lookup!(
    plain_http_is_refused_without_a_lookup,
    format!("git+http://github.com/acme/shared-lib#{SHA}")
);
no_lookup!(
    a_url_with_credentials_is_refused_without_a_lookup,
    format!("git+https://user@github.com/acme/shared-lib#{SHA}")
);
no_lookup!(
    another_git_host_is_refused_without_a_lookup,
    format!("git+https://gitlab.com/acme/shared-lib#{SHA}")
);
no_lookup!(
    a_host_that_only_starts_with_github_is_refused_without_a_lookup,
    format!("git+https://github.com.evil.example/acme/shared-lib#{SHA}")
);
no_lookup!(
    a_github_port_is_refused_without_a_lookup,
    format!("git+https://github.com:444/acme/shared-lib#{SHA}")
);
no_lookup!(
    a_nested_path_is_refused_without_a_lookup,
    format!("git+https://github.com/acme/shared-lib/nested#{SHA}")
);
no_lookup!(
    a_private_registry_is_refused_without_a_lookup,
    "registry+https://private.example/index"
);

#[test]
fn a_public_git_dependency_does_not_admit_a_private_source_beside_it() {
    let mock = serve(public("acme/shared-lib"));
    let sources = [
        ("dep", git("acme/shared-lib", SHA)),
        (
            "secret",
            "git+ssh://git@github.com/acme/private#abc".to_owned(),
        ),
    ];
    let (output, written) = decide(&sources, &mock.url);
    refused(&output, &written);
    assert_eq!(mock.requests().len(), 0);
}
