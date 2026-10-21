//! Image publication roles in the publication workflow IR.

use std::collections::BTreeMap;

use velnor_actions_contract::JobTimeout;
use velnor_actions_workflow_renderer::publish_render::render_publish_workflow;
use velnor_actions_workflow_renderer::publish_spec::{
    PublishJobSpec, PublishPermissions, PublishRole, PublishStep, PublishTriggers,
    PublishWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::PermissionLevel;
use velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext;
use velnor_actions_workflow_renderer::{RenderError, action_step, ambient_shell_step};

const LABEL: &str = "ubuntu-24.04";

fn job(role: PublishRole, needs: &[&str]) -> Result<PublishJobSpec, RenderError> {
    let argv = vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()];
    let privileged = matches!(role, PublishRole::ImagePublish | PublishRole::ImagePromote);
    Ok(PublishJobSpec {
        role,
        display_name: role.as_str().to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        environment: privileged.then(|| "images".to_owned()),
        outputs: BTreeMap::new(),
        permissions: PublishPermissions::expected(role),
        steps: vec![PublishStep {
            id: None,
            step: ambient_shell_step("Work", argv, BTreeMap::new())?,
        }],
        services: Vec::new(),
    })
}

fn rejection(spec: &PublishWorkflowSpec) -> String {
    match spec.validate() {
        Err(RenderError::InvalidWorkflow(text)) => text,
        other => format!("unexpected: {other:?}"),
    }
}

fn image_spec() -> Result<PublishWorkflowSpec, RenderError> {
    let jobs = BTreeMap::from([
        (
            "publish-branch-authority".to_owned(),
            job(PublishRole::BranchAuthority, &[])?,
        ),
        (
            "image-publish-a".to_owned(),
            job(PublishRole::ImagePublish, &["publish-branch-authority"])?,
        ),
    ]);
    Ok(PublishWorkflowSpec {
        name: "Publish".to_owned(),
        repository: "acme/widgets".to_owned(),
        triggers: PublishTriggers {
            branches: vec!["main".to_owned()],
            tag_glob: None,
        },
        jobs,
        staged_binary: None,
    })
}

#[test]
fn image_publish_needs_the_branch_authority_and_a_pinned_environment() -> Result<(), RenderError> {
    let spec = image_spec()?;
    spec.validate()?;
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.28".to_owned(),
        runs_on: LABEL.to_owned(),
    };
    let yaml = render_publish_workflow(&spec, &ctx)?.bytes;
    for scope in [
        "packages: write",
        "attestations: write",
        "id-token: write",
        "actions: read",
        "environment: images",
    ] {
        assert!(yaml.contains(scope), "{scope}:\n{yaml}");
    }
    let mut unpinned = image_spec()?;
    if let Some(job) = unpinned.jobs.get_mut("image-publish-a") {
        job.environment = None;
    }
    assert!(rejection(&unpinned).starts_with("environment_binding:"));
    let mut ungated = image_spec()?;
    if let Some(job) = ungated.jobs.get_mut("image-publish-a") {
        job.needs.clear();
    }
    assert!(rejection(&ungated).starts_with("ungated_job:"));
    Ok(())
}

#[test]
fn image_roles_have_exact_registry_scopes_and_never_check_out() -> Result<(), RenderError> {
    use PermissionLevel::{Read, Write};
    let publish = PublishPermissions::expected(PublishRole::ImagePublish);
    assert_eq!(
        (publish.packages, publish.attestations, publish.id_token),
        (Write, Write, Write)
    );
    assert_eq!((publish.contents, publish.actions), (Read, Read));
    let promote = PublishPermissions::expected(PublishRole::ImagePromote);
    assert_eq!(
        (promote.packages, promote.attestations, promote.id_token),
        (Write, Read, PermissionLevel::None)
    );
    assert_eq!(
        (promote.contents, promote.actions),
        (Read, PermissionLevel::None)
    );
    for role in [PublishRole::Authority, PublishRole::ArtifactBuild] {
        assert_eq!(
            PublishPermissions::expected(role).packages,
            PermissionLevel::None
        );
    }
    let uses = format!("actions/checkout@{:040x}", 1);
    let checkout = PublishStep {
        id: None,
        step: action_step("Checkout", &uses, BTreeMap::new())?,
    };
    let mut leaked = image_spec()?;
    if let Some(job) = leaked.jobs.get_mut("image-publish-a") {
        job.steps.push(checkout);
    }
    assert!(rejection(&leaked).starts_with("checkout_in_privileged_role:"));
    let mut drifted = image_spec()?;
    if let Some(job) = drifted.jobs.get_mut("image-publish-a") {
        job.permissions.packages = PermissionLevel::None;
    }
    assert!(rejection(&drifted).starts_with("permission_matrix:"));
    Ok(())
}
