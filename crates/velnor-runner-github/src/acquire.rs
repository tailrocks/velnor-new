// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::error::WireError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireOutcome {
    Acquired(Vec<i64>),

    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certainty {
    Uncertain,

    Definite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFail {
    Timeout,

    Reset,

    Http(u16),
}

#[must_use]
pub const fn effect_certainty(fail: TransportFail) -> Certainty {
    match fail {
        TransportFail::Timeout | TransportFail::Reset | TransportFail::Http(500..=u16::MAX) => {
            Certainty::Uncertain
        }
        TransportFail::Http(_) => Certainty::Definite,
    }
}

#[must_use]
pub fn sole_grant(returned: &[i64], request_id: i64) -> bool {
    returned == [request_id]
}

pub fn classify_acquire(
    requested: &[i64],
    returned: &[i64],
    already: &[i64],
) -> Result<AcquireOutcome, WireError> {
    let wanted: BTreeSet<i64> = requested.iter().copied().collect();
    if returned.iter().any(|id| !wanted.contains(id)) {
        return Err(WireError::OutsideRequest);
    }
    let got: BTreeSet<i64> = returned.iter().copied().collect();
    let prior: BTreeSet<i64> = already.iter().copied().collect();
    if !got.is_empty() && got == prior {
        return Ok(AcquireOutcome::Noop);
    }
    Ok(AcquireOutcome::Acquired(returned.to_vec()))
}
