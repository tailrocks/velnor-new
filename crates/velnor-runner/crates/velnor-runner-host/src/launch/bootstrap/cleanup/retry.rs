// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use crate::error::HostError;

const OBSERVATION_ATTEMPTS: u32 = 3;

pub(super) async fn while_refused<T, F, Fut, R>(
    mut observe: F,
    mut report: R,
) -> Result<T, HostError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
    R: FnMut(u32, HostError),
{
    let mut attempt = 1;
    loop {
        match observe().await {
            Err(error @ HostError::Identity) if attempt < OBSERVATION_ATTEMPTS => {
                report(attempt, error);
                attempt += 1;
            }
            outcome => return outcome,
        }
    }
}

pub(super) fn log_refusal(attempt: u32, error: HostError) {
    eprintln!(
        "cleanup_observer=refused attempt={attempt} of={OBSERVATION_ATTEMPTS} error={error}"
    );
}

#[cfg(test)]
#[path = "retry/tests.rs"]
mod tests;
