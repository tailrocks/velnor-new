// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use velnor_local::io::env::{Engine, Environment};

use crate::support::temp_dir;

struct Silent;

impl Engine for Silent {
    fn docker(&self, _: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        std::future::ready(Ok(String::new()))
    }

    fn logged(&self, _: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        std::future::ready(Ok(true))
    }
}

#[tokio::test]
async fn down_does_not_stop_a_process_whose_command_only_looks_like_the_authority() {
    let dir = temp_dir("supervisor-lookalike");
    let canonical = std::fs::canonicalize(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let mut decoy: Child = Command::new("/bin/sh")
        .args(["-c", "read line", "supervise", "--dir"])
        .arg(&canonical)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/pid"), decoy.id().to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    let alive = decoy.try_wait().map_or(true, |status| status.is_none());
    let _ = decoy.kill();
    let _ = decoy.wait();
    assert!(down.is_ok(), "{down:?}");
    assert!(alive, "down stopped a process that is not the authority");
}
