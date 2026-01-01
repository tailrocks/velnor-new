//! Container command and application probes for capability admission.

use std::ffi::OsString;
use std::path::Path;

use crate::{CheckDeadline, MiseError};
use velnor_actions_contract_config::config::{
    HostOrbStackSdk, MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES, MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
};

use super::{OrbStackAppObservation, OrbStackObservation, PreparedContainer, invalid, probe};

/// Render a bounded probe failure without disclosing unbounded tool output.
pub(super) fn probe_failure(
    program: &Path,
    args: &[&str],
    result: &crate::command::ProcessOutput,
) -> MiseError {
    let program = program.as_os_str().as_encoded_bytes().escape_ascii();
    let snippet = String::from_utf8_lossy(&result.stderr[..result.stderr.len().min(4 * 1024)]);
    let stdout_digest = velnor_actions_contract::digest_b3(&result.stdout);
    let stderr_digest = velnor_actions_contract::digest_b3(&result.stderr);
    invalid(
        "container_probe",
        format!(
            "bounded_probe_failed: program=\"{program}\", args={args:?}, code={:?}, signal={:?}, stdout_digest={stdout_digest}, stderr_digest={stderr_digest}, stderr={snippet:?}",
            result.code, result.signal,
        ),
    )
}

/// Collect fixed Docker, codesign, plutil, and `OrbStack` CLI observations.
pub(super) fn probe_orbstack(
    sdk: &HostOrbStackSdk,
    prepared: &PreparedContainer,
    env: &[(OsString, OsString)],
    deadline: CheckDeadline,
) -> Result<OrbStackObservation, MiseError> {
    let program = prepared
        .orbctl_program
        .as_ref()
        .ok_or_else(|| invalid("orbstack", "prepared_cli_missing"))?;
    let app = probe_orbstack_app(sdk, program, env, deadline)?;
    Ok(OrbStackObservation {
        program: program.clone(),
        sha256: prepared
            .orbctl_sha256
            .clone()
            .ok_or_else(|| invalid("orbstack", "cli_hash_missing"))?,
        app,
        version: probe(
            program,
            &["version"],
            env,
            deadline,
            MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
        )?,
        status: probe(
            program,
            &["status"],
            env,
            deadline,
            MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
        )?,
    })
}

fn probe_orbstack_app(
    sdk: &HostOrbStackSdk,
    program: &Path,
    env: &[(OsString, OsString)],
    deadline: CheckDeadline,
) -> Result<OrbStackAppObservation, MiseError> {
    let info_path = Path::new(&sdk.app_bundle_path).join("Contents/Info.plist");
    let info_path = info_path
        .to_str()
        .ok_or_else(|| invalid("orbstack", "invalid_plist_path"))?;
    let info = probe(
        Path::new("/usr/bin/plutil"),
        &["-convert", "json", "-o", "-", info_path],
        env,
        deadline,
        MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES,
    )?;
    let codesign = Path::new("/usr/bin/codesign");
    let signature = probe(
        codesign,
        &["-dv", "--verbose=4", &sdk.app_bundle_path],
        env,
        deadline,
        MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES,
    )?;
    let owned_bundle = owned_cli_bundle(program)?;
    let outer_integrity = probe(
        codesign,
        &["--verify", "--strict", &sdk.app_bundle_path],
        env,
        deadline,
        MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES,
    )?;
    let source_cli_integrity = probe(
        codesign,
        &["--verify", "--strict", &sdk.cli_bundle_path],
        env,
        deadline,
        MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES,
    )?;
    let owned_cli_integrity = probe(
        codesign,
        &["--verify", "--strict", owned_bundle],
        env,
        deadline,
        MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES,
    )?;
    let source_cli_signature = probe(
        codesign,
        &["-dv", "--verbose=4", &sdk.cli_bundle_path],
        env,
        deadline,
        MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES,
    )?;
    let owned_cli_signature = probe(
        codesign,
        &["-dv", "--verbose=4", owned_bundle],
        env,
        deadline,
        MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES,
    )?;
    let app = OrbStackAppObservation::parse(
        info,
        signature,
        outer_integrity,
        source_cli_integrity,
        owned_cli_integrity,
        source_cli_signature,
        owned_cli_signature,
    )?;
    Ok(app)
}

fn owned_cli_bundle(program: &Path) -> Result<&str, MiseError> {
    program
        .ancestors()
        .find(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .and_then(Path::to_str)
        .ok_or_else(|| invalid("orbstack", "owned_nested_bundle_missing"))
}

#[cfg(test)]
mod tests;
