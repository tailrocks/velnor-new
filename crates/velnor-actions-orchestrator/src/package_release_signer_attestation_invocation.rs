// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{AttestationRunIdentity, OrchestratorError, internal};

pub(super) fn parse_attestation_invocation(
    value: &str,
    repository_uri: &str,
) -> Result<AttestationRunIdentity, OrchestratorError> {
    if repository_uri
        .strip_prefix("https://github.com/")
        .is_none_or(str::is_empty)
    {
        return Err(internal("package_attestation_repository_invalid"));
    }
    let prefix = format!("{repository_uri}/actions/runs/");
    let identity = value
        .strip_prefix(&prefix)
        .ok_or_else(|| internal("package_attestation_run_identity_mismatch"))?;
    let (run_id, run_attempt) = identity
        .split_once("/attempts/")
        .ok_or_else(|| internal("package_attestation_run_identity_mismatch"))?;
    if run_id.contains('/') || run_attempt.contains('/') {
        return Err(internal("package_attestation_run_identity_mismatch"));
    }
    let run_id = parse_positive_canonical_integer(run_id)?;
    let run_attempt = u32::try_from(parse_positive_canonical_integer(run_attempt)?)
        .map_err(|_| internal("package_attestation_run_identity_mismatch"))?;
    Ok(AttestationRunIdentity {
        run_id,
        run_attempt,
    })
}

fn parse_positive_canonical_integer(value: &str) -> Result<u64, OrchestratorError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| internal("package_attestation_run_identity_mismatch"))?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(internal("package_attestation_run_identity_mismatch"));
    }
    Ok(parsed)
}
