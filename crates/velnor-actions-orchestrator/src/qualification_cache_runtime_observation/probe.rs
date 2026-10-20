use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use velnor_actions_mise::IsolatedCommand;
use velnor_actions_mise::requests::{MiseExecutableLookup, MiseExecutableObservation};

use crate::OrchestratorError;
use crate::internal::internal;

const PROBE_LIMIT: usize = 65_536;
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn probe(selector: &str, payload: &[&str]) -> Result<String, OrchestratorError> {
    let payload = payload.iter().map(OsString::from).collect::<Vec<_>>();
    let command = probe_command(selector, &payload)?;
    let output = command
        .run_bounded(PROBE_LIMIT, PROBE_TIMEOUT)
        .map_err(|_| internal("qualification_runtime_probe_failed"))?;
    output
        .require_success("qualification-runtime-probe")
        .map_err(|_| internal("qualification_runtime_probe_failed"))?;
    let mut text = String::from_utf8(output.stdout)
        .map_err(|_| internal("qualification_runtime_probe_encoding"))?;
    if !output.stderr.is_empty() {
        let stderr = String::from_utf8(output.stderr)
            .map_err(|_| internal("qualification_runtime_probe_encoding"))?;
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if text.is_empty() || text.len() > PROBE_LIMIT || text.contains('\0') || text.contains('\r') {
        return Err(internal("qualification_runtime_probe_output_invalid"));
    }
    Ok(text)
}

fn probe_command(
    selector: &str,
    payload: &[OsString],
) -> Result<IsolatedCommand, OrchestratorError> {
    IsolatedCommand::mise_exec(&[selector.to_owned()], payload)
        .map_err(|_| internal("qualification_runtime_probe_invalid"))
}

pub(super) fn run_lookup(
    lookup: &MiseExecutableLookup,
    selector: &str,
    executable: &str,
    root: &Path,
    args: &[&str],
) -> Result<(MiseExecutableObservation, String), OrchestratorError> {
    let observation = lookup
        .resolve_and_hash(root)
        .map_err(|_| internal("qualification_runtime_probe_failed"))?;
    let mut payload = Vec::with_capacity(args.len() + 1);
    payload.push(OsString::from(executable));
    payload.extend(args.iter().map(OsString::from));
    let command = probe_command(selector, &payload)?;
    let output = command
        .run_bounded(PROBE_LIMIT, PROBE_TIMEOUT)
        .map_err(|_| internal("qualification_runtime_probe_failed"))?;
    output
        .require_success("qualification-runtime-probe")
        .map_err(|_| internal("qualification_runtime_probe_failed"))?;
    let mut text = String::from_utf8(output.stdout)
        .map_err(|_| internal("qualification_runtime_probe_encoding"))?;
    if !output.stderr.is_empty() {
        let stderr = String::from_utf8(output.stderr)
            .map_err(|_| internal("qualification_runtime_probe_encoding"))?;
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if text.is_empty() || text.len() > PROBE_LIMIT || text.contains('\0') || text.contains('\r') {
        return Err(internal("qualification_runtime_probe_output_invalid"));
    }
    let after = lookup
        .resolve_and_hash(root)
        .map_err(|_| internal("qualification_runtime_probe_failed"))?;
    ensure_same_observation(&observation, &after)?;
    Ok((observation, text))
}

fn ensure_same_observation(
    before: &MiseExecutableObservation,
    after: &MiseExecutableObservation,
) -> Result<(), OrchestratorError> {
    if before.path == after.path && before.sha256 == after.sha256 {
        Ok(())
    } else {
        Err(internal(
            "qualification_runtime_executable_changed_during_probe",
        ))
    }
}

pub(super) fn ensure_same_executable(
    observed: &str,
    expected: &str,
) -> Result<(), OrchestratorError> {
    if observed == expected {
        Ok(())
    } else {
        Err(internal("qualification_runtime_executable_changed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_probe_uses_fixed_exact_mise_exec_without_auto_install() {
        let command = probe_command(
            "node@24.21.0",
            &[OsString::from("node"), OsString::from("--version")],
        )
        .expect("fixed probe command");
        assert_eq!(
            command.argv(),
            [
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "exec",
                "node@24.21.0",
                "--",
                "node",
                "--version",
            ]
            .map(OsString::from)
        );
        assert!(command.disables_auto_install());
    }

    #[test]
    fn runtime_probe_rejects_resolved_path_or_digest_drift() {
        let before = MiseExecutableObservation {
            path: Path::new("/mise/installs/node/24.21.0/bin/node").to_owned(),
            sha256: "a".repeat(64),
        };
        assert!(ensure_same_observation(&before, &before).is_ok());
        let changed_path = MiseExecutableObservation {
            path: Path::new("/mise/installs/node/other/bin/node").to_owned(),
            sha256: before.sha256.clone(),
        };
        assert!(ensure_same_observation(&before, &changed_path).is_err());
        let changed_digest = MiseExecutableObservation {
            path: before.path.clone(),
            sha256: "b".repeat(64),
        };
        assert!(ensure_same_observation(&before, &changed_digest).is_err());
    }
}
