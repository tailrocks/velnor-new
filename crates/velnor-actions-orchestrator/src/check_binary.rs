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
    policy: velnor_actions_contract::WorkflowPolicy,
    check: &velnor_actions_mise::DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<ApprovedMiseRuntime, OrchestratorError> {
    resolve_mise_runtime_until(root, policy, check.check.runner.platform, deadline)
}

pub(crate) fn resolve_mise_runtime_for_platform(
    root: &Path,
    policy: velnor_actions_contract::WorkflowPolicy,
    platform: CheckPlatform,
) -> Result<ApprovedMiseRuntime, OrchestratorError> {
    let approved = crate::controller_approval::load_policy_approval(root, policy)?;
    runtime_for_target(approved, platform.release_target())
}

pub(crate) fn resolve_mise_runtime_until(
    root: &Path,
    policy: velnor_actions_contract::WorkflowPolicy,
    platform: CheckPlatform,
    deadline: CheckDeadline,
) -> Result<ApprovedMiseRuntime, OrchestratorError> {
    let approved = crate::controller_approval::load_policy_approval_until(root, policy, deadline)?;
    runtime_for_target(approved, platform.release_target())
}

fn runtime_for_target(
    approved: Option<crate::controller_approval::ControllerApprovedRelease>,
    target: velnor_actions_contract::ReleaseTarget,
) -> Result<ApprovedMiseRuntime, OrchestratorError> {
    if let Some(approved) = approved {
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
    use velnor_actions_contract::config::CheckPlatform;
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

    fn fixture_mise_check(
        platform: CheckPlatform,
    ) -> velnor_actions_contract::config::MiseCheck {
        use velnor_actions_contract::config::{CheckExecutor, CheckRunner, MiseCheck};

        MiseCheck {
            id: "fixture".to_owned(),
            task: "fixture".to_owned(),
            directory: ".".to_owned(),
            runner: CheckRunner {
                label: "fixture".to_owned(),
                platform,
                executor: CheckExecutor::Hosted,
                container: None,
            },
            inputs: Vec::new(),
            tools: Vec::new(),
            system_tools: Vec::new(),
            evidence: None,
            timeout_minutes: 10,
        }
    }

    fn fixture_proposal() -> velnor_actions_contract::propose::ProposedTask {
        use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
        use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};

        ProposedTask {
            task_id: "fixture".to_owned(),
            stack_id: String::new(),
            component_id: String::new(),
            task_kind: String::new(),
            configuration: String::new(),
            depends_on: Vec::new(),
            gated_by: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            outputs: Vec::new(),
            resource: ResourceDemand {
                class: ResourceClass::Lightweight,
                cpu_milli: None,
                memory_mb: None,
                needs_network: false,
                service: None,
            },
            cache_policy: CachePolicy {
                allow_compilation_reuse: false,
                allow_task_reuse: false,
            },
            identity: IdentityInputs {
                unit_id: String::new(),
                unit_key: String::new(),
                unit_path: String::new(),
                project_root: String::new(),
                target: String::new(),
                features: Vec::new(),
                flags: Vec::new(),
                compile_driver: String::new(),
                test_runner: String::new(),
                environment: std::collections::BTreeMap::new(),
                declared_inputs: Vec::new(),
                undeclared_reads: false,
            },
            payload: Vec::new(),
            display_name: String::new(),
            uses_clock: false,
            uses_random: false,
            no_targets: false,
            runner_profile: String::new(),
        }
    }

    fn fixture_check(platform: CheckPlatform) -> velnor_actions_mise::DiscoveredCheck {
        velnor_actions_mise::DiscoveredCheck {
            check: fixture_mise_check(platform),
            proposal: fixture_proposal(),
            config_inputs: Vec::new(),
            task_config: String::new(),
            tool_specs: Vec::new(),
            qualified_tools: Vec::new(),
            qualification_digest: String::new(),
            config_source: String::new(),
        }
    }

    #[test]
    fn runtime_resolution_uses_passed_policy_without_config_read() {
        let dir = std::env::temp_dir().join(format!(
            "velnor-no-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock must advance")
                .as_nanos()
        ));
        std::fs::create_dir(&dir).expect("temp dir must build");
        assert!(
            !dir.join(".velnor/config.toml").is_file(),
            "fixture must not contain a config file"
        );
        let check = fixture_check(CheckPlatform::LinuxX64);
        let target = CheckPlatform::LinuxX64.release_target();
        let runtime = super::resolve_mise_runtime(
            &dir,
            velnor_actions_contract::WorkflowPolicy::ConsumerV1,
            &check,
            velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))
                .expect("test deadline must build"),
        )
        .expect("passed policy must resolve without a config read");
        let pin = velnor_actions_mise::current_mise_binary(target);
        assert_eq!(runtime.target, target);
        assert_eq!(runtime.version, pin.version);
        assert_eq!(runtime.sha256, pin.sha256);
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
