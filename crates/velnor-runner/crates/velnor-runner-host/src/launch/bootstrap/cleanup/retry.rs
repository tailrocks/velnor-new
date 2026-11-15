// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;

use crate::error::HostError;

const OBSERVATION_ATTEMPTS: u32 = 3;

pub(super) async fn while_refused<T, F, Fut>(mut observe: F) -> Result<T, HostError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, HostError>>,
{
    let mut attempt = 1;
    loop {
        match observe().await {
            Err(HostError::Identity) if attempt < OBSERVATION_ATTEMPTS => attempt += 1,
            outcome => return outcome,
        }
    }
}

#[cfg(test)]
#[path = "retry/tests.rs"]
mod tests;
