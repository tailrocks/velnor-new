// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::time::Instant;

use super::JobCancel;

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

#[derive(Clone, Copy)]
pub(super) enum Interruption {
    Cancelled,
    TimedOut,
}

impl Interruption {
    pub(super) fn now(cancel: Option<&JobCancel>, timeout: Option<Timeout>) -> Option<Self> {
        if cancel.is_some_and(JobCancel::is_cancelled) {
            Some(Self::Cancelled)
        } else if timeout.is_some_and(|timeout| Instant::now() >= timeout.at) {
            Some(Self::TimedOut)
        } else {
            None
        }
    }

    pub(super) async fn wait(cancel: Option<&JobCancel>, timeout: Option<Timeout>) -> Self {
        let cancelled = async {
            match cancel {
                Some(cancel) => cancel.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = cancelled => Self::Cancelled,
            () = expired(timeout) => Self::TimedOut,
        }
    }
}
