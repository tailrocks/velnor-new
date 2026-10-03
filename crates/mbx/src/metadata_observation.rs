//! Private request evidence and unqualified metadata observations. No launch capability.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod base64;
mod graph;
mod projection;
mod strict_json;

pub(crate) const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_ENVELOPE_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const MAX_STDOUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ITEMS: usize = 4096;
const MAX_TEXT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalInvocation {
    program: String,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Projection {
    pub(crate) toolchain_selector: Option<String>,
    pub(crate) arguments: Vec<String>,
    /// Evidence checksum only. Does not qualify the producer or response.
    pub(crate) arguments_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MetadataRequest {
    session_uuid: String,
    root_uuid: String,
    nonce: String,
    generation: u8,
    original: OriginalInvocation,
    projection: Projection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationError {
    Bounds,
    InvalidRequest,
    UnsupportedInvocation,
    InvalidJson,
    RequestMismatch,
    InvalidEnvelope,
    InvalidGraph,
}

impl MetadataRequest {
    pub(crate) fn new(
        session_uuid: String,
        root_uuid: String,
        nonce: String,
        generation: u8,
        original: OriginalInvocation,
    ) -> Result<Self, ObservationError> {
        validate_original(&original)?;
        if !uuid(&session_uuid)
            || !uuid(&root_uuid)
            || !(1..=2).contains(&generation)
            || nonce.is_empty()
            || nonce.len() > MAX_TEXT
            || nonce.contains('\0')
        {
            return Err(ObservationError::InvalidRequest);
        }
        let projection = projection::project(&original)?;
        let request = Self {
            session_uuid,
            root_uuid,
            nonce,
            generation,
            original,
            projection,
        };
        if serde_json::to_vec(&request)
            .map_err(|_| ObservationError::InvalidRequest)?
            .len()
            > MAX_REQUEST_BYTES
        {
            return Err(ObservationError::Bounds);
        }
        Ok(request)
    }

    pub(crate) fn projection(&self) -> &Projection {
        &self.projection
    }

    pub(crate) fn binding(&self) -> Result<RequestBinding, ObservationError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ObservationError::InvalidRequest)?;
        Ok(RequestBinding {
            session_uuid: self.session_uuid.clone(),
            root_uuid: self.root_uuid.clone(),
            nonce: self.nonce.clone(),
            generation: self.generation,
            request_digest: projection::digest(&bytes),
        })
    }
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn validate_original(original: &OriginalInvocation) -> Result<(), ObservationError> {
    if !graph::valid_path(&original.program)
        || !graph::valid_path(&original.cwd)
        || original.argv.len() > MAX_ITEMS
        || original.env.len() > MAX_ITEMS
        || original
            .argv
            .iter()
            .any(|arg| !valid_text(arg) || arg.starts_with('@'))
        || original.env.iter().any(|(key, value)| {
            key.is_empty() || key.contains('=') || !valid_text(key) || !valid_text(value)
        })
    {
        return Err(ObservationError::InvalidRequest);
    }
    if serde_json::to_vec(original)
        .map_err(|_| ObservationError::InvalidRequest)?
        .len()
        > MAX_REQUEST_BYTES
    {
        return Err(ObservationError::Bounds);
    }
    Ok(())
}

impl OriginalInvocation {
    pub(crate) fn new(
        program: String,
        argv: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: String,
    ) -> Result<Self, ObservationError> {
        let original = Self {
            program,
            argv,
            env,
            cwd,
        };
        validate_original(&original)?;
        Ok(original)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RequestBinding {
    session_uuid: String,
    root_uuid: String,
    nonce: String,
    generation: u8,
    /// Exact request evidence only; no producer verification.
    request_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDto {
    session_uuid: String,
    root_uuid: String,
    nonce: String,
    generation: u8,
    original: OriginalInvocation,
    projection: Projection,
}

/// Decode bounded request evidence, then rederive projection from the original.
pub(crate) fn parse_request(bytes: &[u8]) -> Result<MetadataRequest, ObservationError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ObservationError::Bounds);
    }
    let dto: RequestDto = serde_json::from_value(strict_json::parse(bytes)?)
        .map_err(|_| ObservationError::InvalidRequest)?;
    let request = MetadataRequest::new(
        dto.session_uuid,
        dto.root_uuid,
        dto.nonce,
        dto.generation,
        dto.original,
    )?;
    if request.projection != dto.projection {
        return Err(ObservationError::RequestMismatch);
    }
    Ok(request)
}

fn valid_text(value: &str) -> bool {
    value.len() <= MAX_TEXT && !value.contains('\0')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(crate) enum TerminalObservation {
    Exited { code: i32 },
    Signaled { signal: u32 },
    SpawnFailed,
    WaitFailed,
    TimedOut,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    request_binding: RequestBinding,
    terminal: TerminalObservation,
    observed_wall_ns: Option<u64>,
    claimed_stdout_eof: bool,
    stdout_base64: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnqualifiedStatus {
    ParsedGraph,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnqualifiedReason {
    /// This parser checks public shape and relationships, not Cargo resolution semantics.
    CargoResolutionNotQualified,
}

/// Parsed claims only. Neither terminal claims nor EOF claims identify a producer.
#[derive(Debug)]
pub(crate) struct UnqualifiedMetadataObservation {
    pub(crate) reason: UnqualifiedReason,
    pub(crate) status: UnqualifiedStatus,
    pub(crate) terminal: TerminalObservation,
    pub(crate) observed_wall_ns: Option<u64>,
    pub(crate) packages: Vec<graph::PackageObservation>,
}

pub(crate) fn parse_observation(
    expected: &MetadataRequest,
    bytes: &[u8],
) -> Result<UnqualifiedMetadataObservation, ObservationError> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(ObservationError::Bounds);
    }
    let value = strict_json::parse(bytes)?;
    let envelope: Envelope =
        serde_json::from_value(value).map_err(|_| ObservationError::InvalidEnvelope)?;
    if envelope.request_binding != expected.binding()? {
        return Err(ObservationError::RequestMismatch);
    }
    let stdout = base64::decode(&envelope.stdout_base64)?;
    validate_terminal(&envelope, &stdout)?;
    let success = envelope.terminal == TerminalObservation::Exited { code: 0 };
    let packages = if success && envelope.claimed_stdout_eof {
        graph::parse(&stdout)?
    } else {
        Vec::new()
    };
    Ok(UnqualifiedMetadataObservation {
        reason: UnqualifiedReason::CargoResolutionNotQualified,
        status: if success && envelope.claimed_stdout_eof {
            UnqualifiedStatus::ParsedGraph
        } else {
            UnqualifiedStatus::Unavailable
        },
        terminal: envelope.terminal,
        observed_wall_ns: envelope.observed_wall_ns,
        packages,
    })
}

fn validate_terminal(envelope: &Envelope, stdout: &[u8]) -> Result<(), ObservationError> {
    let invalid = match envelope.terminal {
        TerminalObservation::Exited { code } => code < 0 || envelope.observed_wall_ns.is_none(),
        TerminalObservation::Signaled { signal } => {
            signal == 0 || envelope.observed_wall_ns.is_none()
        }
        TerminalObservation::SpawnFailed => {
            !stdout.is_empty() || envelope.claimed_stdout_eof || envelope.observed_wall_ns.is_some()
        }
        TerminalObservation::WaitFailed | TerminalObservation::TimedOut => {
            envelope.observed_wall_ns.is_none()
        }
    };
    if invalid {
        Err(ObservationError::InvalidEnvelope)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
