// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GIT_TOKEN_ENV, GIT_TOKEN_REF, ReleaseJobSpec, ReleasePlzAuth, ReleaseRole, RenderError,
    StepKind, isolated_release_plz_argv, release_executable_env, release_rust_archive_step,
    release_rust_prepare, render_release_plz_argv,
};

pub(super) fn release_catalog(rust: &str) -> Result<velnor_actions_mise::ToolCatalog, RenderError> {
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    let base = ToolCatalog::pinned();
    ToolCatalog::new(
        rust,
        base.version(PinnedTool::MrBoxington),
        base.version(PinnedTool::Gh),
        base.version(PinnedTool::Jactionlint),
        base.version(PinnedTool::Shellcheck),
        base.version(PinnedTool::Nextest),
        base.version(PinnedTool::Opentofu),
    )
    .map(|catalog| catalog.with_export_rustup_toolchain(false))
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))
}

pub(super) fn check_rust_custody(
    id: &str,
    job: &ReleaseJobSpec,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    use velnor_actions_contract::StepRole;
    let mut prepare = release_rust_prepare(catalog)?;
    let mut verify = crate::verifier_builders::rust_verifier_shell_step(
        catalog,
        crate::verifier_builders::ToolchainPhase::Installed,
    )?;
    if job.role == ReleaseRole::Preflight {
        let condition = Some("steps.release-admission.outputs.eligible == 'true'".to_owned());
        prepare.condition.clone_from(&condition);
        verify.condition = condition;
    }
    let prepared: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::PreparePinnedTools))
        .collect();
    let verified: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::VerifyRustManifest))
        .collect();
    let expects_prepare = matches!(job.role, ReleaseRole::Preparation | ReleaseRole::Preflight);
    let preparation_with_pr = job.role == ReleaseRole::Preparation && !job.outputs.is_empty();
    let expected_verifications = if preparation_with_pr {
        vec![&verify, &verify]
    } else {
        vec![&verify]
    };
    if (expects_prepare && prepared != vec![&prepare])
        || (!expects_prepare && !prepared.is_empty())
        || verified != expected_verifications
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_custody:{id}"
        )));
    }
    let prepare_index = 3;
    if job.role == ReleaseRole::Preparation
        && (job.steps.get(prepare_index) != Some(&prepare)
            || job.steps.get(prepare_index + 1) != Some(&verify))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_custody_order:{id}"
        )));
    }
    check_release_rust_artifact(id, job, catalog)?;
    check_release_tool_environment(id, job, catalog)
}

fn check_release_rust_artifact(
    id: &str,
    job: &ReleaseJobSpec,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    use velnor_actions_contract::{StepId, StepRole};
    let archives: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ReleaseRustArchive))
        .collect();
    let uploads: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ReleaseRustUpload))
        .collect();
    if job.role != ReleaseRole::Preflight {
        if archives.is_empty() && uploads.is_empty() {
            return Ok(());
        }
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_artifact:{id}"
        )));
    }
    let condition = Some("steps.release-admission.outputs.eligible == 'true'".to_owned());
    let mut archive = release_rust_archive_step(
        catalog.version(velnor_actions_mise::PinnedTool::Rust),
        &job.runs_on,
    )?;
    archive.condition.clone_from(&condition);
    let mut upload = crate::action_step(
        "Upload release Rust toolchain",
        crate::steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            (
                "name".to_owned(),
                "velnor-release-rust-${{ github.run_id }}-${{ github.run_attempt }}".to_owned(),
            ),
            (
                "path".to_owned(),
                "${{ runner.temp }}/velnor-rust-toolchain.tar.gz".to_owned(),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "36".to_owned()),
        ]),
    )?;
    upload.id = Some(StepId::ReleaseRustUpload);
    upload.role = Some(StepRole::ReleaseRustUpload);
    upload.condition = condition;
    if archives != vec![&archive] || uploads != vec![&upload] {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_artifact:{id}"
        )));
    }
    Ok(())
}

fn add_journal_env(
    role: ReleaseRole,
    step: &str,
    expected: &mut std::collections::BTreeMap<String, String>,
) {
    if matches!(step, "Release journal-start" | "Release journal-coordinate") {
        expected.insert("GH_TOKEN".to_owned(), GIT_TOKEN_REF.to_owned());
    }
    if role == ReleaseRole::PublishBootstrap && step == "Release journal-coordinate" {
        expected.insert(
            "CARGO_REGISTRY_TOKEN".to_owned(),
            "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
        );
    }
}

fn release_tool_environment(
    role: ReleaseRole,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<std::collections::BTreeMap<String, String>, RenderError> {
    let mut expected_env = std::collections::BTreeMap::new();
    for (key, value) in velnor_actions_mise::ToolHomes::runner_temp().exec_env(catalog) {
        let key = key
            .into_string()
            .map_err(|_| RenderError::InvalidWorkflow("release_rust_env".to_owned()))?;
        let value = value
            .into_string()
            .map_err(|_| RenderError::InvalidWorkflow("release_rust_env".to_owned()))?;
        expected_env.insert(key, value);
    }
    if role != ReleaseRole::Preparation {
        expected_env.insert(GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned());
    }
    expected_env.insert("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned());
    Ok(expected_env)
}

fn add_retention_env(step: &str, expected: &mut std::collections::BTreeMap<String, String>) {
    if step == "Release verify-preflight-retention" {
        expected.insert(
            "VELNOR_RECEIPT_ARTIFACT_ID".to_owned(),
            "${{ steps.release-receipt-upload.outputs.artifact-id }}".to_owned(),
        );
        expected.insert(
            "VELNOR_RUST_ARTIFACT_ID".to_owned(),
            "${{ steps.release-rust-upload.outputs.artifact-id }}".to_owned(),
        );
    }
}

fn check_release_tool_environment(
    id: &str,
    job: &ReleaseJobSpec,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    let expected_env = release_tool_environment(job.role, catalog)?;
    for step in &job.steps {
        if (step.name.starts_with("Release ")
            || step.name == "Publish release"
            || step.name == "Prepare release PR"
            || step.name == "Validate release")
            && let StepKind::Shell { run, env } = &step.kind
        {
            let mut expected = expected_env.clone();
            add_retention_env(&step.name, &mut expected);
            if step.name == "Release toolchain-acquire" {
                expected.insert(
                    "VELNOR_RUST_ARTIFACT_ID".to_owned(),
                    "${{ needs.release-preflight.outputs.rust_artifact_id }}".to_owned(),
                );
            }
            if matches!(
                step.name.as_str(),
                "Release receipt-acquire" | "Release reconciliation-receipt-acquire"
            ) {
                expected.insert(
                    "VELNOR_RECEIPT_ARTIFACT_ID".to_owned(),
                    "${{ needs.release-preflight.outputs.receipt_artifact_id }}".to_owned(),
                );
            }
            if matches!(
                step.name.as_str(),
                "Publish release" | "Prepare release PR" | "Validate release"
            ) {
                expected.extend(release_executable_env(
                    catalog.version(velnor_actions_mise::PinnedTool::Rust),
                    &job.runs_on,
                )?);
            }
            add_journal_env(job.role, &step.name, &mut expected);
            if *env != expected {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_rust_env:{id}:{}",
                    step.name
                )));
            }
            if matches!(
                step.name.as_str(),
                "Publish release" | "Prepare release PR" | "Validate release"
            ) {
                let auth = match job.role {
                    ReleaseRole::PublishBootstrap => ReleasePlzAuth::Bootstrap,
                    ReleaseRole::PublishOidc => ReleasePlzAuth::Trusted,
                    _ => ReleasePlzAuth::None,
                };
                let prefix = isolated_release_plz_argv(Vec::new(), auth);
                if !run.starts_with(&prefix)
                    || run.get(prefix.len()).map(String::as_str)
                        != Some(crate::release_plz_acquire::BINARY_PATH)
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "release_tool_isolation:{id}:{}",
                        step.name
                    )));
                }
                render_release_plz_argv(run)?;
            }
        }
    }
    Ok(())
}
