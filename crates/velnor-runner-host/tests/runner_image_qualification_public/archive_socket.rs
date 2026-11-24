// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

type TestResult<T> = Result<T, Box<dyn Error>>;

#[test]
fn archive_subcommands_reject_an_embedded_unix_socket_before_admission() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let socket = format!("{}/unix://docker.sock", directory.path().display());
    let path = |name: &str| format!("{}/{name}", directory.path().display());
    let sha = "a".repeat(40);
    let archive_args = [
        directory.path().display().to_string(),
        sha.clone(),
        path("runner.tar"),
        path("dind.tar"),
        path("probe.tar"),
        path("runner.envelope"),
        path("dind.envelope"),
        path("probe.envelope"),
        path("contexts"),
        path("runner.record"),
        path("dind.record"),
        path("probe.record"),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        socket.clone(),
        path("output"),
    ];
    let candidate_args = [
        directory.path().display().to_string(),
        sha,
        path("runner.tar"),
        path("dind.tar"),
        path("probe.tar"),
        path("runner.envelope"),
        path("dind.envelope"),
        path("probe.envelope"),
        path("contexts"),
        socket,
    ];
    for (subcommand, args) in [
        ("qualify-runner-image-archives", archive_args.as_slice()),
        ("check-runner-image-candidate", candidate_args.as_slice()),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
            .arg(subcommand)
            .args(args)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_RUN_ID", "1")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .output()?;
        assert!(!output.status.success(), "{subcommand} accepted the socket");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim(),
            "resource probe release: worker identity",
            "{subcommand} must reject the socket at admission"
        );
    }
    Ok(())
}
