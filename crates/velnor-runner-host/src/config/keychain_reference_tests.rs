// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::time::Instant;

use super::off_runtime;
use crate::error::HostError;

const STALL: Duration = Duration::from_secs(3);
const BUDGET: Duration = Duration::from_millis(200);

#[tokio::test(flavor = "current_thread")]
async fn a_stalled_credential_read_gives_up_at_the_deadline() {
    let started = Instant::now();
    let loaded = off_runtime(started + BUDGET, || {
        std::thread::sleep(STALL);
        Ok(())
    })
    .await;
    assert!(matches!(loaded, Err(HostError::Keychain)));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test(flavor = "current_thread")]
async fn the_controller_thread_keeps_running_while_a_credential_read_stalls() {
    let started = Instant::now();
    let ticker = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        started.elapsed()
    });
    let _ = off_runtime(started + BUDGET, || {
        std::thread::sleep(STALL);
        Ok(())
    })
    .await;
    let ticked = ticker.await.unwrap_or(STALL);
    assert!(ticked < Duration::from_secs(1));
}

#[tokio::test(flavor = "current_thread")]
async fn a_prompt_credential_read_returns_its_bytes() {
    let loaded = off_runtime(Instant::now() + BUDGET, || Ok(vec![7_u8])).await;
    assert_eq!(loaded.ok(), Some(vec![7_u8]));
}
