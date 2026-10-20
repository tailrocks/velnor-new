//! Publication workflow IR cases: authority gate, role matrix, triggers.

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

fn shell(name: &str) -> Result<PublishStep, RenderError> {
    let argv = vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()];
    Ok(PublishStep {
        id: None,
        step: ambient_shell_step(name, argv, BTreeMap::new())?,
    })
}

fn job(role: PublishRole, needs: &[&str]) -> Result<PublishJobSpec, RenderError> {
    let publishing = matches!(role, PublishRole::ArtifactPublish | PublishRole::TagRelease);
    Ok(PublishJobSpec {
        role,
        display_name: role.as_str().to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        environment: publishing.then(|| "release".to_owned()),
        outputs: BTreeMap::new(),
        permissions: PublishPermissions::expected(role),
        steps: vec![shell("Work")?],
    })
}

fn spec() -> Result<PublishWorkflowSpec, RenderError> {
    let jobs = BTreeMap::from([
        (
            "publish-authority".to_owned(),
            job(PublishRole::Authority, &[])?,
        ),
        (
            "bundle-build-a".to_owned(),
            job(PublishRole::ArtifactBuild, &["publish-authority"])?,
        ),
        (
            "bundle-attest-a".to_owned(),
            job(PublishRole::ArtifactAttest, &["bundle-build-a"])?,
        ),
        (
            "bundle-publish-a".to_owned(),
            job(PublishRole::ArtifactPublish, &["bundle-attest-a"])?,
        ),
    ]);
    Ok(PublishWorkflowSpec {
        name: "Publish".to_owned(),
        repository: "acme/widgets".to_owned(),
        triggers: PublishTriggers {
            branches: Vec::new(),
            tag_glob: Some("*-v[0-9]*.[0-9]*.[0-9]*".to_owned()),
        },
        jobs,
        staged_binary: None,
    })
}

fn rejection(spec: &PublishWorkflowSpec) -> String {
    match spec.validate() {
        Err(RenderError::InvalidWorkflow(text)) => text,
        other => format!("unexpected: {other:?}"),
    }
}

#[test]
fn the_complete_chain_validates_and_renders() -> Result<(), RenderError> {
    let spec = spec()?;
    spec.validate()?;
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.28".to_owned(),
        runs_on: LABEL.to_owned(),
    };
    let file = render_publish_workflow(&spec, &ctx)?;
    assert_eq!(file.path, ".github/workflows/publish.yml");
    assert!(file.bytes.contains("group: publish-${{ github.ref }}"));
    Ok(())
}

#[test]
fn every_job_but_the_authority_must_reach_it() -> Result<(), RenderError> {
    let mut missing = spec()?;
    missing.jobs.remove("publish-authority");
    assert_eq!(rejection(&missing), "publish_authority_count");
    let mut ungated = spec()?;
    if let Some(build) = ungated.jobs.get_mut("bundle-build-a") {
        build.needs.clear();
    }
    assert!(rejection(&ungated).starts_with("ungated_job:"));
    let mut backward = spec()?;
    if let Some(build) = backward.jobs.get_mut("bundle-build-a") {
        build.needs = vec!["bundle-publish-a".to_owned()];
    }
    assert!(rejection(&backward).starts_with("backward_need:"));
    let mut twice = spec()?;
    twice
        .jobs
        .insert("second".to_owned(), job(PublishRole::Authority, &[])?);
    assert_eq!(rejection(&twice), "publish_authority_count");
    Ok(())
}

#[test]
fn permissions_must_equal_the_role_matrix() -> Result<(), RenderError> {
    for (id, mutate) in [
        ("publish-authority", PermissionLevel::Write),
        ("bundle-build-a", PermissionLevel::Write),
        ("bundle-attest-a", PermissionLevel::None),
    ] {
        let mut drifted = spec()?;
        if let Some(job) = drifted.jobs.get_mut(id) {
            job.permissions.contents = mutate;
        }
        assert!(
            rejection(&drifted).starts_with("permission_matrix:"),
            "{id}"
        );
    }
    let mut oidc = spec()?;
    if let Some(job) = oidc.jobs.get_mut("bundle-publish-a") {
        job.permissions.id_token = PermissionLevel::Write;
    }
    assert!(rejection(&oidc).starts_with("permission_matrix:"));
    Ok(())
}

#[test]
fn writers_need_an_environment_and_readers_must_not_have_one() -> Result<(), RenderError> {
    let mut unpinned = spec()?;
    if let Some(job) = unpinned.jobs.get_mut("bundle-publish-a") {
        job.environment = None;
    }
    assert!(rejection(&unpinned).starts_with("environment_binding:"));
    let mut pinned = spec()?;
    if let Some(job) = pinned.jobs.get_mut("bundle-attest-a") {
        job.environment = Some("release".to_owned());
    }
    assert!(rejection(&pinned).starts_with("environment_binding:"));
    Ok(())
}

#[test]
fn only_the_build_role_may_check_out_code() -> Result<(), RenderError> {
    let uses = format!("actions/checkout@{:040x}", 1);
    let checkout = PublishStep {
        id: None,
        step: action_step("Checkout", &uses, BTreeMap::new())?,
    };
    let mut build = spec()?;
    if let Some(job) = build.jobs.get_mut("bundle-build-a") {
        job.steps.push(checkout.clone());
    }
    build.validate()?;
    for id in ["publish-authority", "bundle-attest-a", "bundle-publish-a"] {
        let mut leaked = spec()?;
        if let Some(job) = leaked.jobs.get_mut(id) {
            job.steps.push(checkout.clone());
        }
        assert!(
            rejection(&leaked).starts_with("checkout_in_privileged_role:"),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn triggers_are_push_only_and_bounded() -> Result<(), RenderError> {
    let mut none = spec()?;
    none.triggers.tag_glob = None;
    assert_eq!(rejection(&none), "no_publish_trigger");
    let mut bad_branch = spec()?;
    bad_branch.triggers.branches = vec!["../x".to_owned()];
    assert!(rejection(&bad_branch).starts_with("bad_push_branch:"));
    let mut bad_glob = spec()?;
    bad_glob.triggers.tag_glob = Some("v*; rm".to_owned());
    assert!(rejection(&bad_glob).starts_with("bad_tag_glob:"));
    let mut both = spec()?;
    both.triggers.branches = vec!["main".to_owned()];
    both.validate()?;
    Ok(())
}

#[test]
fn job_outputs_must_read_a_declared_step() -> Result<(), RenderError> {
    let mut outputs = spec()?;
    if let Some(job) = outputs.jobs.get_mut("publish-authority") {
        job.outputs.insert(
            "artifact".to_owned(),
            "${{ steps.tag.outputs.artifact }}".to_owned(),
        );
    }
    assert!(rejection(&outputs).starts_with("bad_job_output:"));
    if let Some(job) = outputs.jobs.get_mut("publish-authority") {
        job.steps[0].id = Some("tag".to_owned());
    }
    outputs.validate()?;
    if let Some(job) = outputs.jobs.get_mut("publish-authority") {
        job.outputs
            .insert("x".to_owned(), "${{ secrets.TOKEN }}".to_owned());
    }
    assert!(rejection(&outputs).starts_with("bad_job_output:"));
    Ok(())
}

#[test]
fn conditions_cannot_carry_secret_handles() -> Result<(), RenderError> {
    let mut gated = spec()?;
    if let Some(job) = gated.jobs.get_mut("bundle-build-a") {
        job.condition = Some("secrets.TOKEN != ''".to_owned());
    }
    assert!(rejection(&gated).starts_with("bad_condition:"));
    Ok(())
}

fn release_spec() -> Result<PublishWorkflowSpec, RenderError> {
    let jobs = BTreeMap::from([
        (
            "publish-branch-authority".to_owned(),
            job(PublishRole::BranchAuthority, &[])?,
        ),
        (
            "rust-tag-release".to_owned(),
            job(PublishRole::TagRelease, &["publish-branch-authority"])?,
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
fn the_tag_release_chain_validates_and_may_check_out() -> Result<(), RenderError> {
    let mut spec = release_spec()?;
    spec.validate()?;
    let uses = format!("actions/checkout@{:040x}", 1);
    if let Some(release) = spec.jobs.get_mut("rust-tag-release") {
        release.steps.insert(
            0,
            PublishStep {
                id: None,
                step: action_step("Checkout", &uses, BTreeMap::new())?,
            },
        );
    }
    spec.validate()?;
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.28".to_owned(),
        runs_on: LABEL.to_owned(),
    };
    let file = render_publish_workflow(&spec, &ctx)?;
    assert!(file.bytes.contains("branches:"), "{}", file.bytes);
    assert!(!file.bytes.contains("tags:"), "{}", file.bytes);
    Ok(())
}

#[test]
fn the_tag_release_needs_the_branch_authority_not_the_tag_authority() -> Result<(), RenderError> {
    let mut spec = release_spec()?;
    spec.jobs.insert(
        "publish-authority".to_owned(),
        job(PublishRole::Authority, &[])?,
    );
    if let Some(release) = spec.jobs.get_mut("rust-tag-release") {
        release.needs = vec!["publish-authority".to_owned()];
    }
    assert_eq!(rejection(&spec), "ungated_job:rust-tag-release");
    let mut open = release_spec()?;
    if let Some(release) = open.jobs.get_mut("rust-tag-release") {
        release.permissions.id_token = PermissionLevel::Write;
    }
    assert!(rejection(&open).starts_with("permission_matrix:"));
    let mut unpinned = release_spec()?;
    if let Some(release) = unpinned.jobs.get_mut("rust-tag-release") {
        release.environment = None;
    }
    assert!(rejection(&unpinned).starts_with("environment_binding:"));
    Ok(())
}

#[test]
fn each_authority_role_appears_at_most_once_and_may_be_conditional() -> Result<(), RenderError> {
    let mut spec = release_spec()?;
    if let Some(authority) = spec.jobs.get_mut("publish-branch-authority") {
        authority.condition = Some("github.ref_type == 'branch'".to_owned());
    }
    spec.validate()?;
    spec.jobs.insert(
        "second-branch-authority".to_owned(),
        job(PublishRole::BranchAuthority, &[])?,
    );
    assert_eq!(rejection(&spec), "publish_authority_count");
    Ok(())
}

fn internal(operation: &str) -> PublishStep {
    PublishStep {
        id: None,
        step: velnor_actions_contract::Step {
            parallel: false,
            name: "Gradle mavenBundle".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: velnor_actions_contract::StepKind::Internal {
                operation: operation.to_owned(),
                env: BTreeMap::from([(
                    "VELNOR_GRADLE_TASKS".to_owned(),
                    ":mavenBundle".to_owned(),
                )]),
            },
        },
    }
}

const STAGED: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.28";

fn with_internal(
    role_job: &str,
    operation: &str,
    staged: Option<&str>,
) -> Result<PublishWorkflowSpec, RenderError> {
    let mut spec = spec()?;
    if let Some(job) = spec.jobs.get_mut(role_job) {
        job.steps.push(internal(operation));
    }
    spec.staged_binary = staged.map(str::to_owned);
    Ok(spec)
}

#[test]
fn only_the_build_role_may_run_the_closed_set_of_internal_operations() -> Result<(), RenderError> {
    let ctx = ReleaseRenderContext {
        generator_version: "0.1.28".to_owned(),
        runs_on: LABEL.to_owned(),
    };
    let ok = with_internal("bundle-build-a", "gradle-run-v1", Some(STAGED))?;
    ok.validate()?;
    let yaml = render_publish_workflow(&ok, &ctx)?.bytes;
    assert!(yaml.contains("VELNOR_INTERNAL_OP: gradle-run-v1"), "{yaml}");
    assert!(
        yaml.contains("VELNOR_GRADLE_TASKS: \":mavenBundle\""),
        "{yaml}"
    );
    assert!(yaml.contains("velnor/bin/velnor-actions-0.1.28"), "{yaml}");
    for job in ["bundle-attest-a", "bundle-publish-a", "publish-authority"] {
        let spec = with_internal(job, "gradle-run-v1", Some(STAGED))?;
        assert_eq!(rejection(&spec), format!("publish_internal_op:{job}"));
    }
    for operation in ["plan-v1", "fetch-reports-v1", "merge-v1"] {
        let spec = with_internal("bundle-build-a", operation, Some(STAGED))?;
        assert_eq!(rejection(&spec), "publish_internal_op:bundle-build-a");
    }
    Ok(())
}

#[test]
fn the_staged_helper_exists_exactly_when_an_internal_step_does() -> Result<(), RenderError> {
    let missing = with_internal("bundle-build-a", "gradle-run-v1", None)?;
    assert_eq!(rejection(&missing), "publish_staged_binary");
    let stray = with_internal("bundle-build-a", "gradle-run-v1", Some("/usr/bin/sh"))?;
    assert_eq!(rejection(&stray), "publish_staged_binary");
    let mut unused = spec()?;
    unused.staged_binary = Some(STAGED.to_owned());
    assert_eq!(rejection(&unused), "publish_staged_binary");
    Ok(())
}
