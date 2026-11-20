// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationState {
    Reserved,

    Uncertain,

    Recovering,

    Released,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReservationSnapshot {
    pub(crate) reservation_id: i64,

    pub(crate) state: ReservationState,

    pub(crate) initial_sample_generation_id: Option<String>,

    pub(crate) resume_eligible: bool,

    pub(crate) cpu_millicores: u64,

    pub(crate) memory_bytes: u64,
}

pub(super) fn parse_state(value: &str) -> Result<ReservationState, HostError> {
    match value {
        "reserved" => Ok(ReservationState::Reserved),
        "uncertain" => Ok(ReservationState::Uncertain),
        "recovering" => Ok(ReservationState::Recovering),
        "released" => Ok(ReservationState::Released),
        _ => Err(HostError::Journal),
    }
}
