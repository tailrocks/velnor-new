// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::connect_unix;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn connect_unix_never_rewrites_the_named_socket_path() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    std::fs::write(&socket, b"")?;
    let base = directory.path().display();
    assert!(connect_unix(&format!("{base}/unix://docker.sock")).is_err());
    assert!(connect_unix(&format!("unix://{base}/unix://docker.sock")).is_err());
    assert!(connect_unix(&format!("unix://{base}/docker.sock")).is_ok());
    assert!(connect_unix(&format!("{base}/docker.sock")).is_ok());
    Ok(())
}
