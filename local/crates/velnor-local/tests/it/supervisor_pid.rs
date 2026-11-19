// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::path::Path;

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

fn write_authority_records(dir: &Path, pid: &[u8]) {
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/pid"), pid).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/binary"), "/opt/velnor/bin/velnor-local")
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/address"), "127.0.0.1:9")
        .unwrap_or_else(|e| unreachable!("{e}"));
}

async fn assert_down_keeps_records(dir: &Path) {
    let env = Environment::at(dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    assert!(down.is_err(), "down claimed success: {down:?}");
    for name in ["pid", "binary", "address"] {
        assert!(
            dir.join("authority").join(name).symlink_metadata().is_ok(),
            "{name} was deleted"
        );
    }
}

#[tokio::test]
async fn down_keeps_every_record_when_the_authority_pid_is_empty() {
    let dir = temp_dir("supervisor-pid-empty");
    write_authority_records(&dir, b"");
    assert_down_keeps_records(&dir).await;
}

#[tokio::test]
async fn down_keeps_every_record_when_the_authority_pid_is_not_a_number() {
    let dir = temp_dir("supervisor-pid-text");
    write_authority_records(&dir, b"not-a-pid");
    assert_down_keeps_records(&dir).await;
}

#[tokio::test]
async fn down_keeps_every_record_when_the_authority_pid_cannot_be_read() {
    let dir = temp_dir("supervisor-pid-unreadable");
    write_authority_records(&dir, b"123");
    std::fs::remove_file(dir.join("authority/pid")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::create_dir(dir.join("authority/pid")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    assert!(down.is_err(), "down claimed success: {down:?}");
    assert!(dir.join("authority/binary").exists());
    assert!(dir.join("authority/address").exists());
}

#[tokio::test]
async fn down_keeps_the_host_pid_and_fails_when_it_is_malformed() {
    let dir = temp_dir("supervisor-host-pid-text");
    std::fs::create_dir_all(dir.join("runner")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("runner/pid"), "garbage").unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    assert!(down.is_err(), "down claimed success: {down:?}");
    assert!(dir.join("runner/pid").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn down_keeps_every_record_when_the_authority_pid_is_a_dangling_link() {
    let dir = temp_dir("supervisor-pid-dangling");
    write_authority_records(&dir, b"123");
    std::fs::remove_file(dir.join("authority/pid")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(dir.join("nowhere"), dir.join("authority/pid"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_down_keeps_records(&dir).await;
}
