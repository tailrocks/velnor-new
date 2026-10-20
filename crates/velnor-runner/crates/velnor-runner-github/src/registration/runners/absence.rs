//! A runner registration is a GitHub service record that identifies one runner.
//! This module records metadata and digests for exact registration checks.

use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::session::execute;
use crate::{Method, RunnerReference, SessionError, SessionRequest, Transport, WireError};

use super::{lookup_runner_with_receipt, remove_runner_exchange};

const HEX: &[u8; 16] = b"0123456789abcdef";

/// The operation represented by a runner observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerObservationOperation {
    /// An exact lookup by runner name.
    LookupByName,
    /// A removal request for a runner service ID.
    RemoveById,
}

/// Status of the runner removal operation in an absence receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerRemovalStatus {
    /// The exact lookup already observed no runner with the expected name.
    NotNeeded,
    /// The service accepted deletion and a later lookup observed absence.
    Removed,
    /// The service rejected deletion with a known HTTP status, but a later
    /// lookup independently observed absence.
    AbsentAfterRejectedRemoval,
    /// The deletion result was uncertain, but a later lookup observed absence.
    AbsentAfterUncertainRemoval,
}

/// A runner response receipt records one lookup or removal attempt.
/// It contains response metadata and digests, not response bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerResponseReceipt {
    operation: RunnerObservationOperation,
    status: Option<u16>,
    status_verified: bool,
    count: Option<i64>,
    value_count: Option<usize>,
    request_sha256: String,
    response_sha256: Option<String>,
    observed_at_unix_ms: u128,
}

impl RunnerResponseReceipt {
    pub(super) fn verified(
        operation: RunnerObservationOperation,
        request: &SessionRequest,
        status: u16,
        count: Option<i64>,
        value_count: Option<usize>,
        runner: Option<&RunnerReference>,
    ) -> Result<Self, SessionError> {
        let mut receipt = Self::empty(operation);
        receipt.status = Some(status);
        receipt.status_verified = true;
        receipt.count = count;
        receipt.value_count = value_count;
        receipt.request_sha256 = canonical_request_sha256(request);
        receipt.response_sha256 = Some(canonical_response_sha256(
            operation,
            status,
            count,
            value_count,
            runner,
        ));
        receipt.observed_at_unix_ms = observed_at_unix_ms()?;
        Ok(receipt)
    }

    pub(super) fn empty(operation: RunnerObservationOperation) -> Self {
        Self {
            operation,
            status: None,
            status_verified: false,
            count: None,
            value_count: None,
            request_sha256: String::new(),
            response_sha256: None,
            observed_at_unix_ms: 0,
        }
    }

    pub(super) fn unanswered(
        operation: RunnerObservationOperation,
        request: &SessionRequest,
    ) -> Result<Self, SessionError> {
        Ok(Self {
            request_sha256: canonical_request_sha256(request),
            observed_at_unix_ms: observed_at_unix_ms()?,
            ..Self::empty(operation)
        })
    }

    pub(super) fn delete_status(&mut self, status: u16) {
        self.status = Some(status);
        self.status_verified = status == 204;
        self.response_sha256 = Some(canonical_response_sha256(
            RunnerObservationOperation::RemoveById,
            status,
            None,
            None,
            None,
        ));
    }

    /// Operation that produced this response evidence.
    #[must_use]
    pub const fn operation(&self) -> RunnerObservationOperation {
        self.operation
    }

    /// HTTP status, when the transport received one.
    #[must_use]
    pub const fn status(&self) -> Option<u16> {
        self.status
    }

    /// Whether the response matched the endpoint's accepted success contract.
    #[must_use]
    pub const fn status_verified(&self) -> bool {
        self.status_verified
    }

    /// `count` from a validated lookup response.
    #[must_use]
    pub const fn count(&self) -> Option<i64> {
        self.count
    }

    /// Number of entries in a validated lookup response's `value` array.
    #[must_use]
    pub const fn value_count(&self) -> Option<usize> {
        self.value_count
    }

    /// SHA-256 over length-prefixed method, relative path, optional query, and
    /// body fields. A length-prefixed field has its byte length as an unsigned
    /// 64-bit big-endian value. The query adds `present` and its bytes, or
    /// `absent`. The digest excludes headers, including authentication.
    #[must_use]
    pub fn request_sha256(&self) -> &str {
        &self.request_sha256
    }

    /// SHA-256 over length-prefixed operation and status fields, followed by
    /// presence markers and values for count and value count. A present runner
    /// adds `runner-present`, ID, name, and scale-set ID. If no runner identity
    /// exists, the digest includes `runner-absent`. Delete responses include the
    /// same absence markers. The digest excludes raw response bodies and headers.
    #[must_use]
    pub fn response_sha256(&self) -> Option<&str> {
        self.response_sha256.as_deref()
    }

    /// Unix time in milliseconds recorded for the request attempt. Zero means
    /// no time was recorded. A response need not exist.
    #[must_use]
    pub const fn observed_at_unix_ms(&self) -> u128 {
        self.observed_at_unix_ms
    }
}

/// A runner absence receipt records an exact lookup with no matching registration.
///
/// This receipt does not establish whether the runner had already received a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerAbsenceReceipt {
    runner: RunnerReference,
    removal_status: RunnerRemovalStatus,
    responses: Vec<RunnerResponseReceipt>,
}

impl RunnerAbsenceReceipt {
    /// Exact runner identity whose registration was checked.
    #[must_use]
    pub const fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    /// Result of removal and subsequent absence confirmation.
    #[must_use]
    pub const fn removal_status(&self) -> RunnerRemovalStatus {
        self.removal_status
    }

    /// Ordered attempt evidence. It contains metadata and digests, not bodies.
    #[must_use]
    pub fn responses(&self) -> &[RunnerResponseReceipt] {
        &self.responses
    }
}

/// Check for an exact runner registration.
/// Remove only the expected identity.
///
/// A final empty lookup confirms that the registration was absent at that check.
/// It does not establish whether GitHub assigned a job to the runner earlier.
///
/// # Errors
///
/// An invalid runner identity returns an error.
/// A forbidden removal returns an error before another lookup.
/// After a delete returns 204, a final empty lookup returns `Removed`.
/// After another rejected delete, a final empty lookup returns
/// `AbsentAfterRejectedRemoval`.
/// After an uncertain delete, a final empty lookup returns
/// `AbsentAfterUncertainRemoval`.
/// If the final lookup cannot confirm absence, the function returns an error,
/// including [`SessionError::Uncertain`] when the result is unresolved.
pub fn ensure_runner_absent<T>(
    transport: &mut T,
    expected: &RunnerReference,
    admin_token: &str,
) -> Result<RunnerAbsenceReceipt, SessionError>
where
    T: Transport + ?Sized,
{
    validate_identity(expected)?;
    let (found, lookup) = lookup_runner_with_receipt(transport, &expected.name, admin_token)?;
    let Some(found) = found else {
        return Ok(RunnerAbsenceReceipt {
            runner: expected.clone(),
            removal_status: RunnerRemovalStatus::NotNeeded,
            responses: vec![lookup],
        });
    };
    if !same_identity(&found, expected) {
        return Err(WireError::RegistrationRejected.into());
    }

    let (mut deletion, delete_result) = remove_runner_exchange(transport, expected.id, admin_token);
    let (delete_accepted, delete_rejected) = match delete_result {
        Ok(exchange) => {
            deletion.delete_status(exchange.status);
            if exchange.status == 204 {
                (true, false)
            } else if exchange.status == 403 {
                return Err(WireError::Forbidden.into());
            } else {
                (false, true)
            }
        }
        Err(SessionError::Wire(WireError::Forbidden)) => return Err(WireError::Forbidden.into()),
        Err(_) => (false, false),
    };

    let (remaining, confirmation) =
        lookup_runner_with_receipt(transport, &expected.name, admin_token)?;
    if let Some(remaining) = remaining {
        if same_identity(&remaining, expected) {
            return Err(SessionError::Uncertain);
        }
        return Err(WireError::RegistrationRejected.into());
    }
    let responses = vec![lookup, deletion, confirmation];
    let removal_status = if delete_accepted {
        RunnerRemovalStatus::Removed
    } else if delete_rejected {
        RunnerRemovalStatus::AbsentAfterRejectedRemoval
    } else {
        RunnerRemovalStatus::AbsentAfterUncertainRemoval
    };
    Ok(RunnerAbsenceReceipt {
        runner: expected.clone(),
        removal_status,
        responses,
    })
}

fn validate_identity(runner: &RunnerReference) -> Result<(), SessionError> {
    if runner.id <= 0 || runner.runner_scale_set_id <= 0 || runner.name.is_empty() {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(())
}

fn same_identity(left: &RunnerReference, right: &RunnerReference) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.runner_scale_set_id == right.runner_scale_set_id
}

pub(super) fn exchange_receipt<T>(
    transport: &mut T,
    request: &SessionRequest,
    operation: RunnerObservationOperation,
) -> (RunnerResponseReceipt, Result<crate::Exchange, SessionError>)
where
    T: Transport + ?Sized,
{
    let unanswered = RunnerResponseReceipt::unanswered(operation, request);
    match unanswered {
        Ok(mut receipt) => {
            let result = execute(transport, request);
            if let Ok(exchange) = &result
                && operation == RunnerObservationOperation::RemoveById
            {
                receipt.delete_status(exchange.status);
            }
            match observed_at_unix_ms() {
                Ok(observed_at) => {
                    receipt.observed_at_unix_ms = observed_at;
                    (receipt, result)
                }
                Err(error) => (receipt, Err(error)),
            }
        }
        Err(error) => (RunnerResponseReceipt::empty(operation), Err(error)),
    }
}

fn canonical_request_sha256(request: &SessionRequest) -> String {
    let method = match request.method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
        Method::Patch => "PATCH",
    };
    let mut canonical = Vec::new();
    append_field(&mut canonical, method.as_bytes());
    append_field(&mut canonical, request.path.as_bytes());
    append_optional_field(&mut canonical, request.query.clone());
    append_field(&mut canonical, &request.body);
    sha256(&canonical)
}

fn canonical_response_sha256(
    operation: RunnerObservationOperation,
    status: u16,
    count: Option<i64>,
    value_count: Option<usize>,
    runner: Option<&RunnerReference>,
) -> String {
    let mut canonical = Vec::new();
    let operation = match operation {
        RunnerObservationOperation::LookupByName => "lookup-by-name",
        RunnerObservationOperation::RemoveById => "remove-by-id",
    };
    append_field(&mut canonical, operation.as_bytes());
    append_field(&mut canonical, status.to_string().as_bytes());
    append_optional_field(&mut canonical, count.map(|value| value.to_string()));
    append_optional_field(&mut canonical, value_count.map(|value| value.to_string()));
    match runner {
        Some(runner) => {
            append_field(&mut canonical, b"runner-present");
            append_field(&mut canonical, runner.id.to_string().as_bytes());
            append_field(&mut canonical, runner.name.as_bytes());
            append_field(
                &mut canonical,
                runner.runner_scale_set_id.to_string().as_bytes(),
            );
        }
        None => append_field(&mut canonical, b"runner-absent"),
    }
    sha256(&canonical)
}

fn append_optional_field(target: &mut Vec<u8>, value: Option<String>) {
    match value {
        Some(value) => {
            append_field(target, b"present");
            append_field(target, value.as_bytes());
        }
        None => append_field(target, b"absent"),
    }
}

fn append_field(target: &mut Vec<u8>, value: &[u8]) {
    target.extend_from_slice(&(value.len() as u64).to_be_bytes());
    target.extend_from_slice(value);
}

fn sha256(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn observed_at_unix_ms() -> Result<u128, SessionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| SessionError::Uncertain)
}
