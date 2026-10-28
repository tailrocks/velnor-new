// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use crate::internal::internal;
use std::path::Path;
use velnor_actions_contract::config::CheckPlatform;
use velnor_actions_mise::catalog::lock::ApprovedMiseRuntime;
use velnor_actions_mise::CheckDeadline;

pub(crate) fn resolve_mise_runtime(
    root: &Path,
    check: &velnor_actions_mise::DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<ApprovedMiseRuntime, OrchestratorError> {
    let policy = crate::config::load_config(root)?.workflow.policy;
    let target = check.check.runner.platform.release_target();
    if let Some(approved) =
        crate::controller_approval::load_policy_approval_until(root, policy, deadline)?
    {
        return select_runtime_for_target(&approved, target)
            .ok_or_else(|| internal("mise_runtime_missing_target"));
    }
    let pin = velnor_actions_mise::current_mise_binary(target);
    Ok(ApprovedMiseRuntime {
        target,
        version: pin.version.to_owned(),
        sha256: pin.sha256.to_owned(),
        bytes: pin.bytes,
    })
}

fn select_runtime_for_target(
    approved: &crate::controller_approval::ControllerApprovedRelease,
    target: velnor_actions_contract::ReleaseTarget,
) -> Option<ApprovedMiseRuntime> {
    approved
        .as_authority()
        .mise_runtime
        .iter()
        .find(|runtime| runtime.target == target)
        .cloned()
}

pub(super) fn project_mise_binary(
    home: &Path,
    platform: CheckPlatform,
    runtime: &ApprovedMiseRuntime,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if runtime.target != platform.release_target() {
        return Err(internal("mise_runtime_target_mismatch"));
    }
    let source = velnor_actions_mise::command::resolve_check_program("mise")
        .map_err(|e| internal(&e.to_string()))?;
    project_binary_until(
        Path::new(&source),
        &home.join("bin/mise"),
        &runtime.sha256,
        runtime.bytes,
        Some(deadline),
    )
}

fn project_binary_until(
    source: &Path,
    destination: &Path,
    expected: &str,
    max_bytes: u64,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    let bytes =
        crate::retrieve_reports::staged_reads::read_staged_bytes_until(source, max_bytes, || {
            read_deadline_checkpoint(deadline)
        })
        .map_err(|_| internal("mise_binary_unreadable"))?;
    verify_binary_bytes(&bytes, expected)?;
    crate::exclusive_write::write_exclusive_until(
        destination,
        &bytes,
        "qualified_mise_binary",
        || deadline_checkpoint(deadline),
    )?;
    executable_readonly(destination, deadline)
}

fn verify_binary_bytes(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    if bytes.is_empty() || crate::generator::sha256_hex(bytes) != expected {
        return Err(internal("mise_binary_unqualified_sha256"));
    }
    Ok(())
}

fn executable_readonly(
    path: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    deadline_checkpoint(deadline)?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("mise_binary_permissions"))?;
    rustix::fs::fchmod(&fd, rustix::fs::Mode::RUSR | rustix::fs::Mode::XUSR)
        .map_err(|_| internal("mise_binary_permissions"))?;
    deadline_checkpoint(deadline)
}

fn deadline_checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| internal(&error.to_string()))?;
    }
    Ok(())
}

fn read_deadline_checkpoint(deadline: Option<CheckDeadline>) -> Result<(), &'static str> {
    deadline.map_or(Ok(()), |value| {
        value
            .remaining()
            .map(|_| ())
            .map_err(|_| "deadline_exhausted")
    })
}

#[cfg(test)]
mod tests {
    use super::project_mise_binary;
    use std::path::Path;
    use velnor_actions_contract::ReleaseTarget;
    use velnor_actions_mise::catalog::lock::ApprovedMiseRuntime;

    fn runtime_for(target: ReleaseTarget) -> ApprovedMiseRuntime {
        ApprovedMiseRuntime {
            target,
            version: "2026.10.19".to_owned(),
            sha256: "0".repeat(64),
            bytes: 1,
        }
    }

    #[test]
    fn mismatched_runtime_target_fails_before_touching_the_filesystem() {
        let home = Path::new("/nonexistent-check-home");
        let runtime = runtime_for(ReleaseTarget::MacosArm64);
        let error = project_mise_binary(
            home,
            velnor_actions_contract::config::CheckPlatform::LinuxX64,
            &runtime,
            velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))
                .expect("test deadline must build"),
        )
        .expect_err("wrong-target runtime must fail");
        assert!(
            format!("{error:?}").contains("mise_runtime_target_mismatch"),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn selected_runtime_uses_approved_identity_not_compiled_pin() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let approvals =
            crate::controller_approval::ControllerApprovedRelease::from_approvals_text(&text)
                .expect("shipped approvals must parse");
        let selected = super::select_runtime_for_target(&approvals, ReleaseTarget::LinuxX86_64)
            .expect("linux runtime must resolve");
        assert_eq!(selected.version, "2026.10.19");
        assert_eq!(
            selected.sha256,
            "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d"
        );
        assert_ne!(
            selected.sha256,
            velnor_actions_mise::MISE_BINARY_SHA256_LINUX_X64,
            "selection must not return the compiled bootstrap identity"
        );
    }

    #[test]
    fn consumer_policy_skips_approval_loading_without_approval_file() {
        let dir = std::env::temp_dir().join(format!(
            "velnor-no-approval-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock must advance")
                .as_nanos()
        ));
        std::fs::create_dir(&dir).expect("temp dir must build");
        let resolved = crate::controller_approval::load_policy_approval_until(
            &dir,
            velnor_actions_contract::WorkflowPolicy::ConsumerV1,
            velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))
                .expect("test deadline must build"),
        )
        .expect("consumer policy must resolve");
        assert!(resolved.is_none());
        std::fs::remove_dir(&dir).expect("temp dir must clean");
    }

    #[test]
    fn approved_runtime_table_covers_every_check_platform() {
        let path = format!(
            "{}/../../.velnor/controller-approvals.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).expect("approvals file must exist");
        let approvals =
            crate::controller_approval::ControllerApprovedRelease::from_approvals_text(&text)
                .expect("shipped approvals must parse");
        let authority = approvals.as_authority();
        for target in ReleaseTarget::ALL {
            assert!(
                authority
                    .mise_runtime
                    .iter()
                    .any(|runtime| runtime.target == target),
                "approved runtime lacks {}",
                target.triple()
            );
        }
    }
}
