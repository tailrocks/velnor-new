use bollard::models::ContainerInspectResponse;
use serde::{Deserialize, Serialize};

use crate::error::HostError;

use super::{MAX_RECORD_BYTES, bounded_text};

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnosticRecord {
    pub(super) version: u8,
    pub(super) connection_id: String,
    pub(super) intent_id: i64,
    pub(super) claim_generation: i64,
    pub(super) role: String,
    pub(super) container_id: String,
    pub(super) name: String,
    pub(super) image: Option<String>,
    pub(super) restart_count: Option<i64>,
    pub(super) state: Option<DiagnosticState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnosticState {
    pub(super) status: Option<String>,
    pub(super) running: Option<bool>,
    pub(super) paused: Option<bool>,
    pub(super) restarting: Option<bool>,
    pub(super) oom_killed: Option<bool>,
    pub(super) dead: Option<bool>,
    pub(super) exit_code: Option<i64>,
    pub(super) started_at: Option<String>,
    pub(super) finished_at: Option<String>,
}

#[cfg(test)]
pub(crate) struct CaptureIdentity<'a> {
    pub(super) connection_id: String,
    pub(super) intent_id: i64,
    pub(super) generation: i64,
    pub(super) role: &'static str,
    pub(super) expected_name: &'a str,
    pub(super) container_id: &'a str,
    pub(super) volume: &'a str,
}

pub(crate) struct RecordIdentity<'a> {
    pub(super) connection_id: String,
    pub(super) intent_id: i64,
    pub(super) generation: i64,
    pub(super) role: &'static str,
    pub(super) expected_name: &'a str,
    pub(super) container_id: &'a str,
}

#[cfg(test)]
pub(crate) fn diagnostic_record(
    info: ContainerInspectResponse,
    identity: &CaptureIdentity<'_>,
) -> Result<DiagnosticRecord, HostError> {
    let id = info.id.as_deref().ok_or(HostError::Ownership)?;
    let name = info.name.as_deref().ok_or(HostError::Ownership)?;
    let labels = info
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .ok_or(HostError::Ownership)?;
    let actual_name = name.strip_prefix('/').unwrap_or(name);
    if id != identity.container_id
        || actual_name != identity.expected_name
        || labels.get("velnor.volume").map(String::as_str) != Some(identity.volume)
        || labels.get("velnor.worker").map(String::as_str) != Some(identity.volume)
        || labels.get("velnor.role").map(String::as_str) != Some(identity.role)
    {
        return Err(HostError::Ownership);
    }
    diagnostic_record_fields(
        info,
        &RecordIdentity {
            connection_id: identity.connection_id.clone(),
            intent_id: identity.intent_id,
            generation: identity.generation,
            role: identity.role,
            expected_name: identity.expected_name,
            container_id: identity.container_id,
        },
    )
}

pub(crate) fn diagnostic_record_fields(
    info: ContainerInspectResponse,
    identity: &RecordIdentity<'_>,
) -> Result<DiagnosticRecord, HostError> {
    let id = info.id.as_deref().ok_or(HostError::Ownership)?;
    let name = info.name.as_deref().ok_or(HostError::Ownership)?;
    let actual_name = name.strip_prefix('/').unwrap_or(name);
    if id != identity.container_id || actual_name != identity.expected_name {
        return Err(HostError::Ownership);
    }
    let state = info.state.map(|state| DiagnosticState {
        status: state.status.map(|status| status.to_string()),
        running: state.running,
        paused: state.paused,
        restarting: state.restarting,
        oom_killed: state.oom_killed,
        dead: state.dead,
        exit_code: state.exit_code,
        started_at: bounded_text(state.started_at, 64).filter(|value| valid_timestamp(value)),
        finished_at: bounded_text(state.finished_at, 64).filter(|value| valid_timestamp(value)),
    });
    let record = DiagnosticRecord {
        version: 1,
        connection_id: identity.connection_id.clone(),
        intent_id: identity.intent_id,
        claim_generation: identity.generation,
        role: identity.role.to_owned(),
        container_id: id.to_owned(),
        name: identity.expected_name.to_owned(),
        image: bounded_text(info.image, 128).filter(|value| valid_image_digest(value)),
        restart_count: info.restart_count,
        state,
    };
    let bytes = serde_json::to_vec(&record).map_err(|_| HostError::Journal)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(HostError::Ownership);
    }
    Ok(record)
}

fn valid_image_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(is_lower_hex))
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || !digits(&bytes[0..4])
        || bytes[4] != b'-'
        || !digits(&bytes[5..7])
        || bytes[7] != b'-'
        || !digits(&bytes[8..10])
        || bytes[10] != b'T'
        || !digits(&bytes[11..13])
        || bytes[13] != b':'
        || !digits(&bytes[14..16])
        || bytes[16] != b':'
        || !digits(&bytes[17..19])
    {
        return false;
    }

    let year = number(&bytes[0..4]);
    let month = number(&bytes[5..7]);
    let day = number(&bytes[8..10]);
    let hour = number(&bytes[11..13]);
    let minute = number(&bytes[14..16]);
    let second = number(&bytes[17..19]);
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => return false,
    };
    if day == 0 || day > month_days || hour > 23 || minute > 59 || second > 60 {
        return false;
    }

    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fraction_start || index - fraction_start > 9 {
            return false;
        }
    }
    let Some(zone) = bytes.get(index..) else {
        return false;
    };
    if zone == b"Z" {
        return true;
    }
    zone.len() == 6
        && matches!(zone[0], b'+' | b'-')
        && digits(&zone[1..3])
        && zone[3] == b':'
        && digits(&zone[4..6])
        && number(&zone[1..3]) <= 23
        && number(&zone[4..6]) <= 59
}

fn digits(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_digit)
}

fn number(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |value, byte| {
        value * 10 + u16::from(byte.saturating_sub(b'0'))
    })
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}
