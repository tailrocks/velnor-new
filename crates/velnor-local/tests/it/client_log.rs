// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::{Future, ready};
use std::io;
use std::path::Path;
use std::process::Stdio;

use serde_json::json;
use velnor_local::io::env::{Engine, Environment, client_log_tail};
use velnor_local::scenario::{Metrics, Outcome};
use velnor_local::verdict::{Check, verdict};

use crate::support::temp_dir;

const TEMPLATE_ERROR: &str = "Unexpected value 'cache-mode'";
const FAKE_RUNNER_CLIENT: &str = r#"echo "Runner.Client: workflow template is invalid: Unexpected value 'cache-mode'"; echo "Runner.Client: exiting" >&2; exit 1"#;

#[derive(Clone, Copy)]
struct FakeRunnerClient;

impl Engine for FakeRunnerClient {
    fn docker(&self, _: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        ready(Ok(String::new()))
    }

    fn logged(&self, _: &[&str], log: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        let log = log.to_path_buf();
        async move {
            let file = std::fs::File::create(&log)?;
            let status = tokio::process::Command::new("sh")
                .args(["-c", FAKE_RUNNER_CLIENT])
                .stdin(Stdio::null())
                .stdout(file.try_clone()?)
                .stderr(file)
                .status()
                .await?;
            Ok(status.success())
        }
    }
}

#[tokio::test]
async fn a_runner_client_that_exits_non_zero_puts_its_error_in_the_verdict() {
    let dir = temp_dir("client-log-error");
    let env = Environment::at(&dir, FakeRunnerClient)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let log = dir.join("client.log");
    let succeeded = env
        .client(&json!({}), &[], &log)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!succeeded);

    let tail = client_log_tail(&log)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = Outcome {
        id: 7,
        workflow: "ci".to_owned(),
        event: "push".to_owned(),
        conclusion: "unfinished",
        started: None,
        metrics: Metrics::Failed("run 7 is pending".to_owned()),
        caches: Vec::new(),
        trusted: true,
        client_log: Some(tail),
    };
    let text = verdict("leaf", &[outcome], (0, 1.0), &Check::Plain, "");
    assert!(text.contains(TEMPLATE_ERROR), "{text}");
    assert!(text.contains("Runner.Client: exiting"), "{text}");
}
