// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::super::{Prepared, TestResult};
use velnor_runner_github::{
    Exchange, Method, QueueSession, SessionRequest, Transport, TransportFail,
};

const DIRECTORY: &str = "VELNOR_JIT_INBOX_FAKE";

pub(super) fn isolated(name: &str) -> TestResult<bool> {
    if std::env::var_os(DIRECTORY).is_some() {
        return Ok(false);
    }
    let directory = tempfile::tempdir()?;
    let executable = directory.path().join("curl");
    let bun = bun_executable()?;
    let (_, source) = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/launch/jit_conflict/tests/inbox_fake.ts"
    ))
    .split_once('\n')
    .ok_or("fake curl script shebang")?;
    std::fs::write(&executable, format!("#!{}\n{source}", bun.display()))?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    let mut paths = vec![directory.path().to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").ok_or("PATH")?,
    ));
    let (_, name) = name.split_once("::").ok_or("qualified test name")?;
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", name, "--nocapture"])
        .env(DIRECTORY, directory.path())
        .env("PATH", std::env::join_paths(paths)?)
        .output()?;
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stderr().write_all(&output.stderr)?;
    assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    assert!(
        output.status.success(),
        "isolated inbox recovery test failed: {}",
        output.status
    );
    Ok(true)
}

fn bun_executable() -> TestResult<PathBuf> {
    let direct = Command::new("bun")
        .args(["--print", "process.execPath"])
        .output();
    let output = match direct {
        Ok(output) if output.status.success() => output,
        _ => {
            let version = std::env::var("VELNOR_TOOL_BUN")
                .map_err(|_| "inbox fake requires Bun on PATH or installed VELNOR_TOOL_BUN")?;
            Command::new("mise")
                .args([
                    "--no-config",
                    "--no-env",
                    "--no-hooks",
                    "which",
                    "--tool",
                    &format!("bun@{version}"),
                    "bun",
                ])
                .env("MISE_AUTO_INSTALL", "false")
                .env("MISE_EXEC_AUTO_INSTALL", "false")
                .output()?
        }
    };
    if !output.status.success() {
        return Err(format!(
            "inbox fake could not locate installed Bun: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let path = String::from_utf8(output.stdout)?;
    let path = path.trim();
    let executable = PathBuf::from(path);
    if !executable.is_absolute() || !executable.is_file() || path.chars().any(char::is_whitespace) {
        return Err(format!("invalid inbox fake Bun executable: {path:?}").into());
    }
    Ok(executable)
}

pub(super) async fn poll(case: &Prepared, body: &str) -> TestResult<()> {
    let directory = PathBuf::from(std::env::var_os(DIRECTORY).ok_or("fake directory")?);
    std::fs::write(directory.join("body"), body)?;
    std::fs::write(directory.join("requests"), "")?;
    let mut external = Github;
    let config = crate::config::HostConfig::parse(super::super::super::HOST_CONFIG)?;
    let binding = config.connections().first().ok_or("binding")?;
    let mut link = link(&mut external, binding)?;
    let mut session = session()?;
    let nonce = "e".repeat(32);
    phase(
        case.scope.prepare_session_create(&nonce).await,
        "prepare poll session",
    )?;
    let claim = phase(
        case.scope.claim_session_create(&nonce).await,
        "claim poll session",
    )?
    .ok_or("session claim")?;
    let bound = phase(
        case.scope
            .record_created_session(&claim, &session.session_id, 0)
            .await,
        "bind poll session",
    )?;
    let outcome = phase(
        crate::launch::poll_scoped_connection_once(
            &mut link,
            7,
            &mut session,
            &case.scope,
            bound.intent_id(),
            bound.allocation(),
            0,
        )
        .await,
        "real scoped poll",
    )?;
    assert_eq!(outcome.message_id, Some(80));
    let requests = std::fs::read_to_string(directory.join("requests"))?;
    assert!(requests.contains("GET https://queue.example.com/messages?"));
    assert!(requests.contains("DELETE https://queue.example.com/messages/80"));
    Ok(())
}

fn session() -> TestResult<QueueSession> {
    struct Queue;
    impl Transport for Queue {
        fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
            assert_eq!(request.method, Method::Post);
            assert!(request.path.ends_with("/7/sessions"));
            Ok(Exchange {
                status: 200,
                body: br#"{"sessionId":"inbox-session","messageQueueUrl":"https://queue.example.com/messages","messageQueueAccessToken":"queue-token"}"#.to_vec(),
            })
        }
    }
    Ok(velnor_runner_github::create_session(
        &mut Queue,
        7,
        "inbox-owner",
        "admin",
    )?)
}

pub(super) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::digest_hex::lowercase_hex(&Sha256::digest(bytes))
}

fn phase<T, E: std::fmt::Display>(result: Result<T, E>, name: &str) -> TestResult<T> {
    result.map_err(|error| format!("bootstrap inbox {name}: {error}").into())
}

struct Github;

fn link(
    external: &mut Github,
    binding: &crate::config::RepositoryConnection,
) -> TestResult<crate::listen::Link> {
    let admin = velnor_runner_github::admin_connection(
        external,
        &velnor_runner_github::AdminConnectionCall {
            config_url: "https://github.com/owner/repo",
            registration_token: "registration",
        },
    )?;
    let scope = velnor_runner_github::workflow_run::get_repository_scope(
        external,
        velnor_runner_github::workflow_run::RepositoryTarget {
            id: 42,
            owner: "owner",
            name: "repo",
        },
        "pat",
    )?;
    Ok(crate::listen::link_from_admin(
        crate::https::HttpsTransport::new("https://api.github.com")?,
        admin,
        binding,
        &scope,
    )?)
}

impl Transport for Github {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let body = match (&request.method, request.path.as_str()) {
            (Method::Post, "/actions/runner-registration") => {
                serde_json::json!({"url":"https://admin.example.com","token":"admin"})
            }
            (Method::Get, "/repos/owner/repo") => {
                serde_json::json!({"id":42,"name":"repo","full_name":"owner/repo","private":true,"owner":{"login":"owner"}})
            }
            _ => panic!("unexpected external GitHub request {request:?}"),
        };
        Ok(Exchange {
            status: 200,
            body: serde_json::to_vec(&body).expect("GitHub response"),
        })
    }
}
