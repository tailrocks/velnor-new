//! Publication IR cases for Sigstore-bundle signing with the existing
//! `ArtifactAttest` role: fixed permissions, no checkout, and the npm push
//! after its provenance record job.

use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::publish_render::render_publish_workflow;
use velnor_actions_workflow_renderer::publish_scripts::maven::attest_disabled_script;
use velnor_actions_workflow_renderer::publish_spec::{
    PublishPermissions, PublishRole, PublishStep, PublishWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::PermissionLevel;
use velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext;
use velnor_actions_workflow_renderer::{RenderError, action_step};

use crate::impl_renderer_publish_spec::{LABEL, job, rejection, spec};

/// `spec()` with the declared Sigstore-bundle shape on the existing attest role.
fn signed_spec() -> Result<PublishWorkflowSpec, RenderError> {
    let mut spec = spec()?;
    spec.jobs.insert(
        "bundle-attest-a".to_owned(),
        job(PublishRole::ArtifactAttest, &["bundle-build-a"])?,
    );
    Ok(spec)
}

#[test]
fn sigstore_signing_uses_the_fixed_artifact_attest_role_permissions() -> Result<(), RenderError> {
    let sign = PublishPermissions::expected(PublishRole::ArtifactAttest);
    assert_eq!(sign.contents, PermissionLevel::Read);
    assert_eq!(sign.id_token, PermissionLevel::Write);
    assert_eq!(sign.attestations, PermissionLevel::Write);
    assert_eq!(sign.packages, PermissionLevel::None);
    let spec = signed_spec()?;
    spec.validate()?;
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.28".to_owned(),
        runs_on: LABEL.to_owned(),
    };
    let file = render_publish_workflow(&spec, &ctx)?;
    let body = file
        .bytes
        .split("  bundle-attest-a:")
        .nth(1)
        .and_then(|rest| rest.split("  bundle-build-a:").next())
        .unwrap_or_default();
    assert!(body.contains("id-token: write"), "{body}");
    assert!(body.contains("attestations: write"), "{body}");
    let mut store = signed_spec()?;
    if let Some(job) = store.jobs.get_mut("bundle-attest-a") {
        job.permissions.id_token = PermissionLevel::None;
    }
    assert!(rejection(&store).starts_with("permission_matrix:"));
    let uses = format!("actions/checkout@{:040x}", 1);
    let mut leaked = signed_spec()?;
    if let Some(job) = leaked.jobs.get_mut("bundle-attest-a") {
        job.steps.push(PublishStep {
            id: None,
            step: action_step("Checkout", &uses, BTreeMap::new())?,
        });
    }
    assert!(rejection(&leaked).starts_with("checkout_in_privileged_role:"));
    Ok(())
}

#[test]
fn the_npm_push_may_follow_a_provenance_record_job() -> Result<(), RenderError> {
    let mut spec = signed_spec()?;
    let mut push = job(PublishRole::NpmPublish, &["bundle-publish-a"])?;
    push.environment = Some("release".to_owned());
    spec.jobs.insert("npm-publish-a".to_owned(), push);
    spec.validate()?;
    let mut backward = spec.clone();
    if let Some(record) = backward.jobs.get_mut("bundle-publish-a") {
        record.needs = vec!["npm-publish-a".to_owned()];
    }
    assert!(rejection(&backward).starts_with("backward_need:"));
    Ok(())
}

#[test]
fn maven_attestation_opt_out_emits_one_warning_line() {
    let script = attest_disabled_script("private repository without artifact attestations");
    assert!(!script.contains('\n'), "{script:?}");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("run warning script");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"::warning title=Provenance attestation disabled::publish.maven attest=false: private repository without artifact attestations\n"
    );
}
