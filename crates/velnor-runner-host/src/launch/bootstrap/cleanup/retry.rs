// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    future::Future,
    time::{Duration, Instant},
};

use crate::error::HostError;

const OBSERVATION_ATTEMPTS: u32 = 3;

pub(super) async fn while_refused<T, F, Fut, R>(
    window: Duration,
    mut observe: F,
    mut report: R,
) -> Result<T, HostError>
where
    F: FnMut(Instant) -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
    R: FnMut(u32, HostError),
{
    let deadline = Instant::now() + window;
    let mut attempt = 1;
    loop {
        match observe(deadline).await {
            Err(error @ HostError::Identity) => {
                report(attempt, error);
                if attempt >= OBSERVATION_ATTEMPTS {
                    return Err(error);
                }
                attempt += 1;
            }
            outcome => return outcome,
        }
    }
}

pub(super) fn log_refusal(attempt: u32, error: HostError) {
    eprintln!("cleanup_observer=refused attempt={attempt} of={OBSERVATION_ATTEMPTS} error={error}");
}

#[cfg(test)]
#[path = "retry/tests.rs"]
mod tests;
