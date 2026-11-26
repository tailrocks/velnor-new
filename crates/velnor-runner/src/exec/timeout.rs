// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::time::Instant;

#[derive(Clone, Copy)]
pub(super) struct Timeout {
    pub(super) minutes: f64,
    at: Instant,
}

impl Timeout {
    pub(super) fn start(minutes: Option<f64>) -> Option<Self> {
        let minutes = minutes?;
        let limit = Duration::try_from_secs_f64(minutes * 60.0).ok()?;
        Some(Self {
            minutes,
            at: Instant::now().checked_add(limit)?,
        })
    }
}

pub(super) async fn expired(timeout: Option<Timeout>) {
    match timeout {
        Some(timeout) => tokio::time::sleep_until(timeout.at).await,
        None => std::future::pending().await,
    }
}
