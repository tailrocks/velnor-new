// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use uuid::Uuid;

use super::inputs::saturating;

const ATTEMPTS: u32 = 5;
const RETRY_MULTIPLIER: f64 = 1.5;

pub(super) struct Failure {
    pub(super) error: anyhow::Error,
    pub(super) retryable: bool,
    pub(super) after: Option<Duration>,
}

impl Failure {
    pub(super) fn fatal(error: impl Into<anyhow::Error>) -> Self {
        Self {
            error: error.into(),
            retryable: false,
            after: None,
        }
    }

    pub(super) fn transient(error: impl Into<anyhow::Error>) -> Self {
        Self {
            error: error.into(),
            retryable: true,
            after: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Backoff {
    base: Duration,
    jitter: bool,
    budget: Option<Duration>,
}

fn base(default: Duration, variable: &str) -> Duration {
    if cfg!(feature = "test-support") {
        std::env::var(variable)
            .ok()
            .and_then(|text| text.parse::<u64>().ok())
            .map_or(default, Duration::from_millis)
    } else {
        default
    }
}

pub(super) fn rpc() -> Backoff {
    Backoff {
        base: base(Duration::from_secs(8), "VELNOR_ARTIFACT_RPC_BASE_MS"),
        jitter: true,
        budget: Some(Duration::from_secs(120)),
    }
}

pub(super) fn blob() -> Backoff {
    Backoff {
        base: base(Duration::from_secs(1), "VELNOR_ARTIFACT_BLOB_BASE_MS"),
        jitter: false,
        budget: None,
    }
}

impl Backoff {
    fn delay(self, retry: u32) -> Duration {
        if retry == 0 {
            return self.base;
        }
        let low =
            self.base.as_secs_f64() * RETRY_MULTIPLIER.powi(i32::try_from(retry).unwrap_or(0));
        let spread = if self.jitter {
            let unit = f64::from(u32::try_from(Uuid::new_v4().as_u128() % 1_000_000).unwrap_or(0))
                / 1_000_000.0;
            low * (RETRY_MULTIPLIER - 1.0) * unit
        } else {
            0.0
        };
        Duration::try_from_secs_f64(low + spread).unwrap_or(self.base)
    }
}

pub(super) type Notes = Mutex<Vec<String>>;

#[derive(Clone, Copy)]
pub(super) enum Mode<'a> {
    Rpc(&'a Notes),
    BlobDownload,
    BlobUpload,
}

pub(super) async fn attempts<T, F, Fut>(
    backoff: Backoff,
    mode: Mode<'_>,
    mut attempt: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, Failure>>,
{
    let mut waited = Duration::ZERO;
    let mut retry = 0_u32;
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(failure) if !failure.retryable => return Err(failure.error),
            Err(failure) if retry + 1 >= ATTEMPTS => {
                return Err(match mode {
                    Mode::Rpc(_) => failure
                        .error
                        .context(format!("Failed to make request after {ATTEMPTS} attempts")),
                    Mode::BlobDownload => {
                        anyhow::anyhow!("Artifact download failed after {ATTEMPTS} retries.")
                    }
                    Mode::BlobUpload => failure.error,
                });
            }
            Err(failure) => {
                let wait = failure.after.unwrap_or_else(|| backoff.delay(retry));
                if let Mode::Rpc(notes) = mode {
                    if let Some(budget) = backoff
                        .budget
                        .filter(|budget| waited.saturating_add(wait) > *budget)
                    {
                        return Err(failure.error.context(format!(
                            "Retry wait of {} ms would exceed the maximum total retry wait of {} ms",
                            wait.as_millis(),
                            budget.as_millis()
                        )));
                    }
                    if let Ok(mut notes) = notes.lock() {
                        notes.push(format!(
                            "Attempt {} of {ATTEMPTS} failed with error: {:#}. Retrying request in {} ms...",
                            retry + 1,
                            failure.error,
                            wait.as_millis()
                        ));
                    }
                }
                tokio::time::sleep(wait).await;
                waited = waited.saturating_add(wait);
                retry += 1;
            }
        }
    }
}

pub(super) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let text = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    saturating(text, u64::MAX)
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
}
