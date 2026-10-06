//! Closed actual OCI phases and source/receipt environments, never generic overrides.
use super::{NativePublishRole, invalid};
use crate::{ContractError, HelperInvocation, SourceBoundOperation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn operations(role: &NativePublishRole) -> Result<(), ContractError> {
    let b = role.binding();
    match role {
        NativePublishRole::DesktopTagZip { .. } => {
            if b.admission.descriptor().operation() != SourceBoundOperation::NativePublishAdmission
                || b.full_ci_admission.descriptor().operation()
                    != SourceBoundOperation::NativePublishAdmission
                || b.receipt.descriptor().operation()
                    != SourceBoundOperation::NativePublishReceiptVerifier
            {
                return Err(invalid("invalid_compiled_binding"));
            }
        }
        NativePublishRole::DockerHubIndex { .. } => {
            let ci = b.full_ci_admission.args().get(2).map_or("", String::as_str);
            if !matches!(ci, "ci.yml" | ".github/workflows/ci.yml")
                || !phase(
                    &b.full_ci_admission,
                    &["verify", &b.repository, ci, &b.default_branch],
                )
                || !phase(
                    &b.admission,
                    &["publish-admission", &b.repository, ci, &b.default_branch],
                )
                || !phase(&b.receipt, &["index-receipt", "-", "-", "-"])
            {
                return Err(invalid("foreign_oci_phase_or_source"));
            }
        }
    }
    Ok(())
}
fn phase(invocation: &HelperInvocation, args: &[&str]) -> bool {
    invocation.descriptor().operation() == SourceBoundOperation::OciDelivery
        && invocation
            .args()
            .iter()
            .map(String::as_str)
            .eq(args.iter().copied())
}
pub(super) fn allows_preparation(
    role: &NativePublishRole,
    operation: SourceBoundOperation,
) -> bool {
    match role {
        NativePublishRole::DesktopTagZip { .. } => matches!(
            operation,
            SourceBoundOperation::MiseBootstrap
                | SourceBoundOperation::NativePublishToolPreparation
        ),
        NativePublishRole::DockerHubIndex { .. } => matches!(
            operation,
            SourceBoundOperation::MiseBootstrap | SourceBoundOperation::MiseToolPrepare
        ),
    }
}
pub(super) fn environment(role: &NativePublishRole) -> Result<(), ContractError> {
    let NativePublishRole::DockerHubIndex {
        environment: e,
        subject_name,
        ..
    } = role
    else {
        return Ok(());
    };
    let b = role.binding();
    let canonical = if e.image.starts_with("docker.io/") {
        e.image.clone()
    } else {
        format!("docker.io/{}", e.image)
    };
    let platforms = e
        .platforms
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if canonical != *subject_name
        || e.image_id.is_empty()
        || e.image_id.len() > 128
        || !e
            .image_id
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase())
        || !e
            .image_id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || b.artifact_job != format!("image-{}", e.image_id)
        || platforms.is_empty()
        || platforms.len() != e.platforms.len()
        || platforms.iter().any(|p| !matches!(*p, "amd64" | "arm64"))
    {
        return Err(invalid("foreign_oci_image_or_platforms"));
    }
    check_source(&e.full_ci, true)?;
    check_source(&e.admission, false)?;
    check_source(&e.receipt, false)?;
    for (key, expected) in [
        ("IMAGE", e.image.clone()),
        ("IMAGE_ID", e.image_id.clone()),
        (
            "VERSION",
            format!("${{{{ needs.{}.outputs.version }}}}", b.full_ci_job),
        ),
        (
            "PLATFORMS",
            platforms.into_iter().collect::<Vec<_>>().join(","),
        ),
        ("ARTIFACT_JOB", b.artifact_job.clone()),
        (
            "ARTIFACT_ID",
            format!(
                "${{{{ needs.{}.outputs.{} }}}}",
                b.artifact_job, b.artifact_id_output
            ),
        ),
        (
            "ARTIFACT_DIGEST",
            format!(
                "sha256:${{{{ needs.{}.outputs.{} }}}}",
                b.artifact_job, b.artifact_digest_output
            ),
        ),
    ] {
        if e.receipt.get(key) != Some(&expected) {
            return Err(invalid("foreign_oci_receipt_binding"));
        }
    }
    check_extra(&e.full_ci, false)?;
    check_extra(&e.admission, false)?;
    check_extra(&e.receipt, true)
}
fn check_source(environment: &BTreeMap<String, String>, verify: bool) -> Result<(), ContractError> {
    let mut expected = BTreeMap::from([
        ("GH_TOKEN", "${{ github.token }}"),
        ("REF", "${{ github.ref }}"),
        ("SOURCE_SHA", "${{ github.sha }}"),
        ("REPOSITORY", "${{ github.repository }}"),
        ("GITHUB_RUN_ID", "${{ github.run_id }}"),
        ("GITHUB_RUN_ATTEMPT", "${{ github.run_attempt }}"),
        ("GITHUB_REPOSITORY", "${{ github.repository }}"),
        ("GITHUB_REF", "${{ github.ref }}"),
        ("GITHUB_EVENT_NAME", "${{ github.event_name }}"),
    ]);
    if verify || environment.contains_key("EVENT_NAME") {
        expected.insert("EVENT_NAME", "${{ github.event_name }}");
    }
    if expected
        .iter()
        .any(|(key, value)| environment.get(*key).map(String::as_str) != Some(*value))
    {
        return Err(invalid("foreign_oci_source_environment"));
    }
    Ok(())
}
fn check_extra(environment: &BTreeMap<String, String>, receipt: bool) -> Result<(), ContractError> {
    for (key, value) in environment {
        if matches!(
            key.as_str(),
            "GH_TOKEN"
                | "REF"
                | "SOURCE_SHA"
                | "REPOSITORY"
                | "GITHUB_RUN_ID"
                | "GITHUB_RUN_ATTEMPT"
                | "GITHUB_REPOSITORY"
                | "GITHUB_REF"
                | "GITHUB_EVENT_NAME"
                | "EVENT_NAME"
        ) {
            continue;
        }
        if receipt
            && matches!(
                key.as_str(),
                "IMAGE"
                    | "IMAGE_ID"
                    | "VERSION"
                    | "PLATFORMS"
                    | "ARTIFACT_JOB"
                    | "ARTIFACT_ID"
                    | "ARTIFACT_DIGEST"
            )
        {
            continue;
        }
        if !safe_sdk_environment(key, value) {
            return Err(invalid("foreign_oci_credential_or_environment"));
        }
    }
    Ok(())
}
fn safe_sdk_environment(key: &str, value: &str) -> bool {
    match key {
        "MISE_NO_CONFIG" | "MISE_NO_ENV" | "MISE_NO_HOOKS" | "PYTHONNOUSERSITE" => value == "1",
        "MISE_LOCKFILE" => value == "0",
        "MISE_AUTO_INSTALL" | "MISE_EXEC_AUTO_INSTALL" => value == "false",
        "MISE_DATA_DIR" => value == "${{ runner.temp }}/velnor/mise",
        "MISE_CONFIG_DIR" => value == "${{ runner.temp }}/velnor/mise-config",
        "MISE_CACHE_DIR" => value == "${{ runner.temp }}/velnor/mise-cache",
        "MISE_STATE_DIR" => value == "${{ runner.temp }}/velnor/mise-state",
        "HOME" => value == "${{ runner.temp }}/velnor/delivery-home",
        "PATH" => value == "/usr/bin:/bin:/usr/sbin:/sbin",
        "RUNNER_TEMP" => value == "${{ runner.temp }}",
        "VELNOR_QUALIFIED_TOOL_IDENTITY" => {
            value
                .strip_prefix("qualified-tools@")
                .is_some_and(|digest| {
                    !digest.is_empty()
                        && digest.bytes().all(|c| {
                            c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':')
                        })
                })
        }
        _ => false,
    }
}

pub(super) fn version_output(
    role: &NativePublishRole,
    proof: &crate::Job,
) -> Result<(), ContractError> {
    if !matches!(role, NativePublishRole::DockerHubIndex { .. }) {
        return Ok(());
    }
    let b = role.binding();
    if !proof.outputs.iter().any(|output| {
        output.name == "version"
            && output.value.output == crate::workflow::ActionOutput::ReleaseVersion
            && proof.steps.iter().any(|step| step.id.as_ref() == Some(&output.value.step_id)
                && matches!(&step.kind, crate::StepKind::SourceBoundHelper { invocation, .. } if invocation == &b.full_ci_admission))
    }) { return Err(invalid("missing_oci_version_binding")); }
    Ok(())
}
