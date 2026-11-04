// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::{ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig};
use velnor_actions_contract::{ReleaseTarget, StepId, StepKind};
use velnor_actions_mise::{MISE_VERSION, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::release_gates::{ReleaseConfigBinding, check_release_jobs};
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_permissions::PermissionLevel;
use velnor_actions_workflow_renderer::release_spec::{
    DispatchInput, ReleasePlan, ReleaseTriggers, lock::stable_lock_group, publish_gate_condition,
};
use velnor_actions_workflow_renderer::{MiseSetup, setup::MISE_BINARY_SHA256_LINUX_X64};

use super::{JobInputs, assemble_jobs};

fn consumer_manifest(generator_version: &str) -> String {
    let commit = "b".repeat(40);
    let digest = "a".repeat(64);
    let targets = ReleaseTarget::ALL.iter().map(|target| {
        let target = target.triple();
        format!("{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{generator_version}/velnor-actions-{generator_version}-{target}\",\"sha256\":\"{digest}\"}}")
    }).collect::<Vec<_>>().join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{generator_version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
    )
}

pub(crate) fn fixture() -> Result<(ReleaseWorkflowSpec, String), Box<dyn std::error::Error>> {
    fixture_for(ReleaseAuthentication::TrustedPublishing)
}

fn fixture_for(authentication: ReleaseAuthentication) -> Result<(ReleaseWorkflowSpec, String), Box<dyn std::error::Error>> {
    fixture_for_release_pr(authentication, true)
}

fn fixture_for_release_pr(authentication: ReleaseAuthentication, release_pr: bool) -> Result<(ReleaseWorkflowSpec, String), Box<dyn std::error::Error>> {
    let generator_version = env!("CARGO_PKG_VERSION");
    let repository = "owner/project";
    let source = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let plan = ReleasePlan {
        plan_id: "release-aaaaaaaaaaaa".to_owned(),
        repository: repository.to_owned(),
        source_sha: source.to_owned(),
        registry: "crates-io".to_owned(),
        packages: BTreeMap::from([("example".to_owned(), "0.1.1".to_owned())]),
        version: Some("0.1.1".to_owned()),
        dispatch: true,
    };
    let release = RustReleaseConfig {
        enabled: true,
        registry: ReleaseRegistry::CratesIo,
        authentication: Some(authentication),
        release_pr,
        packages: vec!["example".to_owned()],
        required_workflow_path: ".github/workflows/ci.yml".to_owned(),
        trusted_reviewer_id: 1,
        ..RustReleaseConfig::default()
    };
    let catalog = ToolCatalog::pinned().with_export_rustup_toolchain(false);
    let mise = MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(generator_version, "ubuntu-24.04", &"b".repeat(40), &"c".repeat(64))?;
    let jobs = assemble_jobs(&JobInputs {
        release: &release,
        gate: publish_gate_condition(repository, "main", &plan, authentication == ReleaseAuthentication::TrustedPublishing),
        preparation_gate: format!(
            "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/main' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'"
        ),
        registry: None,
        label: "ubuntu-24.04",
        mise: &mise,
        catalog: &catalog,
        acquire: &acquire,
        generator_version,
    })?;
    let dispatch_inputs = [
        ("plan", plan.plan_id.as_str()),
        ("source_sha", source),
        ("version", "0.1.1"),
    ]
    .into_iter()
    .map(|(name, value)| DispatchInput {
        name: name.to_owned(),
        description: name.to_owned(),
        required: true,
        default: Some(value.to_owned()),
    })
    .collect();
    Ok((
        ReleaseWorkflowSpec {
            name: "Velnor Release".to_owned(),
            repository: repository.to_owned(),
            triggers: ReleaseTriggers {
                push_branches: vec!["main".to_owned()],
                schedule: None,
                dispatch_inputs,
            },
            concurrency: stable_lock_group("crates-io", repository, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")?,
            jobs,
            plan,
            publish_environment: "release".to_owned(),
            bootstrap_environment: "release".to_owned(),
            journal_predecessor: None,
            new_crates: None,
            release_plz_version: catalog.version(PinnedTool::ReleasePlz).to_owned(),
        },
        catalog.version(PinnedTool::Rust).to_owned(),
    ))
}

fn valid(spec: &ReleaseWorkflowSpec, rust: &str) -> bool {
    spec.validate().is_ok()
        && check_release_jobs(
            spec,
            &ReleaseConfigBinding {
                generator_version: env!("CARGO_PKG_VERSION"),
                release_plz_version: &spec.release_plz_version,
                rust_version: rust,
            },
        )
        .is_ok()
}

#[test]
fn release_pr_preparation_separates_source_execution_from_write_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let producer = spec.jobs.get("release-preparation").ok_or("producer")?;
    let writer = spec.jobs.get("release-pr-create").ok_or("missing create-only writer")?;
    assert_eq!(producer.permissions.contents, PermissionLevel::Read);
    assert_eq!(producer.permissions.pull_requests, PermissionLevel::None);
    assert_eq!(producer.permissions.id_token, PermissionLevel::None);
    assert_eq!(writer.permissions.contents, PermissionLevel::Write);
    assert_eq!(writer.permissions.pull_requests, PermissionLevel::Write);
    assert_eq!(writer.permissions.actions, PermissionLevel::Read);
    assert_eq!(writer.permissions.id_token, PermissionLevel::None);
    assert_eq!(writer.needs, ["release-preparation"]);
    assert!(writer.steps.iter().all(|step| !matches!(
        &step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")
    )));
    assert!(producer.steps.iter().filter_map(|step| match &step.kind {
        StepKind::Action { uses, with, .. } if uses.starts_with("actions/checkout@") => Some(with),
        _ => None,
    }).all(|with| with.get("persist-credentials").is_some_and(|value| value == "false")));
    let ctx = velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(&spec, &ctx, &rust)?;
    assert!(workflow.contains("release-pr-create:"));
    assert!(workflow.contains("needs.release-preparation.outputs.artifact_id"));
    Ok(())
}

#[test]
fn release_pr_split_rejects_authority_and_artifact_forgery() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    assert!(valid(&spec, &rust));
    let mut candidate = spec.clone();
    candidate.jobs.get_mut("release-preparation").ok_or("producer")?.permissions.contents = PermissionLevel::Write;
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let producer = candidate.jobs.get_mut("release-preparation").ok_or("producer")?;
    producer.steps[1] = velnor_actions_workflow_renderer::action_step("Checkout", crate::workflow::CHECKOUT_USES, BTreeMap::from([("persist-credentials".into(), "true".into()), ("fetch-depth".into(), "0".into())]))?;
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let producer = candidate.jobs.get_mut("release-preparation").ok_or("producer")?;
    let step = producer.steps.iter_mut().find(|step| step.name == "Prepare release PR candidate").ok_or("candidate")?;
    let StepKind::Shell { env, .. } = &mut step.kind else { return Err("candidate shell".into()) };
    env.insert("GIT_TOKEN".into(), "${{ secrets.GITHUB_TOKEN }}".into());
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    candidate.jobs.get_mut("release-preparation").ok_or("producer")?.outputs.insert("artifact_id".into(), "${{ steps.release-pr-artifact.outputs.artifact-digest }}".into());
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let producer = candidate.jobs.get_mut("release-preparation").ok_or("producer")?;
    producer.steps.swap(8, 9);
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    candidate.jobs.get_mut("release-pr-create").ok_or("writer")?.condition = None;
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    candidate.jobs.get_mut("release-pr-create").ok_or("writer")?.permissions.id_token = PermissionLevel::Write;
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let checkout = candidate.jobs.get("release-preparation").ok_or("producer")?.steps[1].clone();
    candidate.jobs.get_mut("release-pr-create").ok_or("writer")?.steps.push(checkout);
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let writer = candidate.jobs.get_mut("release-pr-create").ok_or("writer")?;
    let StepKind::Shell { env, .. } = &mut writer.steps[1].kind else { return Err("writer shell".into()) };
    env.insert("VELNOR_PR_CONTENT_SHA256".into(), "${{ needs.release-preparation.outputs.artifact_digest }}".into());
    assert!(!valid(&candidate, &rust));
    Ok(())
}

#[test]
fn disabled_release_pr_keeps_readonly_dry_run_without_writer() -> Result<(), Box<dyn std::error::Error>> {
    for authentication in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for_release_pr(authentication, false)?;
        assert!(valid(&spec, &rust));
        assert!(!spec.jobs.contains_key("release-pr-create"));
        let producer = spec.jobs.get("release-preparation").ok_or("producer")?;
        assert_eq!(producer.permissions.contents, PermissionLevel::Read);
        assert!(producer.outputs.is_empty());
        assert!(producer.steps.iter().any(|step| step.name == "Validate release"));
    }
    Ok(())
}

#[test]
fn rust_artifact_custody_must_precede_publishing() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    assert!(
        valid(&spec, &rust),
        "generated release should satisfy custody"
    );
    let mut absent = spec.clone();
    absent
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .steps
        .retain(|step| step.name != "Verify Rust channel manifest");
    assert!(
        !valid(&absent, &rust),
        "publisher must verify the toolchain"
    );
    let mut reordered = spec.clone();
    let job = reordered
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?;
    let acquire = job
        .steps
        .iter()
        .position(|step| step.name == "Release toolchain-acquire")
        .ok_or("acquire")?;
    let verify = job
        .steps
        .iter()
        .position(|step| step.name == "Verify Rust channel manifest")
        .ok_or("verify")?;
    job.steps.swap(acquire, verify);
    assert!(
        !valid(&reordered, &rust),
        "verification must follow acquisition"
    );
    let mut missing_id = spec.clone();
    missing_id
        .jobs
        .get_mut("release-preflight")
        .ok_or("preflight")?
        .outputs
        .remove("rust_artifact_id");
    assert!(
        !valid(&missing_id, &rust),
        "artifact id must be bound to preflight"
    );
    let mut forged_id = spec.clone();
    let upload = forged_id
        .jobs
        .get_mut("release-preflight")
        .ok_or("preflight")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Upload release Rust toolchain")
        .ok_or("upload")?;
    upload.id = Some(StepId::ReleaseAdmission);
    assert!(
        !valid(&forged_id, &rust),
        "artifact upload identity cannot be forged"
    );
    Ok(())
}

#[test]
fn semver_tool_is_required_before_preparation_and_candidate_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    assert!(valid(&spec, &rust));
    for id in ["release-preparation", "release-preflight", "release-publish"] {
        let mut missing = spec.clone();
        let job = missing.jobs.get_mut(id).ok_or("job")?;
        let index = job.steps.iter().position(|step| step.name == "Acquire cargo-semver-checks").ok_or("semver acquisition")?;
        job.steps.remove(index);
        assert!(!valid(&missing, &rust), "{id} accepted a missing checker");
        let mut changed = spec.clone();
        let job = changed.jobs.get_mut(id).ok_or("job")?;
        let step = job.steps.iter_mut().find(|step| step.name == "Acquire cargo-semver-checks").ok_or("semver acquisition")?;
        if let velnor_actions_contract::StepKind::Shell { run, .. } = &mut step.kind {
            run[2] = "true".to_owned();
        } else { return Err("expected shell step".into()); }
        assert!(!valid(&changed, &rust), "{id} accepted an unverified checker");
    }
    Ok(())
}

#[test]
fn consumer_and_reconciliation_depend_on_verified_preflight()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let mut forged_consumer = spec.clone();
    let acquire = forged_consumer
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Release toolchain-acquire")
        .ok_or("acquire")?;
    let StepKind::Shell { env, .. } = &mut acquire.kind else {
        return Err("acquire is not a shell step".into());
    };
    env.insert(
        "VELNOR_RUST_ARTIFACT_ID".to_owned(),
        "${{ inputs.artifact_id }}".to_owned(),
    );
    assert!(
        !valid(&forged_consumer, &rust),
        "publisher must use the preflight artifact"
    );
    let mut missing_dependency = spec.clone();
    missing_dependency
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .needs
        .clear();
    assert!(
        !valid(&missing_dependency, &rust),
        "publisher must depend on preflight"
    );
    let mut reconcile_without_verification = spec.clone();
    reconcile_without_verification
        .jobs
        .get_mut("release-reconcile")
        .ok_or("reconcile")?
        .steps
        .retain(|step| step.name != "Verify Rust channel manifest");
    assert!(
        !valid(&reconcile_without_verification, &rust),
        "reconciliation must verify its toolchain"
    );
    Ok(())
}

#[test]
fn consumers_acquire_the_receipt_bound_to_preflight() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let preflight = spec.jobs.get("release-preflight").ok_or("preflight")?;
    let position = |name: &str| preflight.steps.iter().position(|step| step.name == name);
    assert!(
        position("Release source-admit") < position("Checkout exact source")
            && position("Checkout exact source") < position("Prepare pinned tools")
            && position("Prepare pinned tools") < position("Verify Rust channel manifest")
            && position("Verify Rust channel manifest") < position("Release candidate-receipt")
            && position("Release candidate-receipt") < position("Upload release receipt")
            && position("Upload release receipt") < position("Archive release Rust toolchain"),
        "source admission and Rust verification must precede workspace receipt"
    );
    assert!(position("Release verify-package").is_none(), "preflight must not verify a package before dependency publication");
    assert_eq!(
        preflight.outputs.get("receipt_artifact_id").map(String::as_str),
        Some("${{ steps.release-receipt-upload.outputs.artifact-id }}"),
        "preflight must expose the actual receipt upload ID"
    );
    for consumer in ["release-publish", "release-reconcile"] {
        let job = spec.jobs.get(consumer).ok_or("consumer")?;
        assert!(job.needs.iter().any(|need| need == "release-preflight"));
        let operation = if consumer == "release-reconcile" { "reconciliation-receipt-acquire" } else { "receipt-acquire" };
        let acquire = job.steps.iter().find(|step| step.name == format!("Release {operation}")).ok_or("receipt acquisition")?;
        let StepKind::Shell { run, env } = &acquire.kind else {
            return Err("receipt acquisition is not a shell step".into());
        };
        assert_eq!(run.last().map(String::as_str), Some(operation));
        assert_eq!(env.get("VELNOR_RECEIPT_ARTIFACT_ID").map(String::as_str), Some("${{ needs.release-preflight.outputs.receipt_artifact_id }}"));
        assert!(!job.steps.iter().any(|step| step.name == "Download release receipt"));
    }
    assert!(valid(&spec, &rust));
    let mut forged_id = spec.clone();
    let acquire = forged_id.jobs.get_mut("release-publish").ok_or("publisher")?.steps.iter_mut().find(|step| step.name == "Release receipt-acquire").ok_or("acquire")?;
    let StepKind::Shell { env, .. } = &mut acquire.kind else { return Err("acquire shell".into()); };
    env.insert("VELNOR_RECEIPT_ARTIFACT_ID".to_owned(), "${{ inputs.artifact_id }}".to_owned());
    assert!(!valid(&forged_id, &rust), "receipt artifact must come from this preflight");

    let mut altered_upload = spec.clone();
    let upload = altered_upload.jobs.get_mut("release-preflight").ok_or("preflight")?.steps.iter_mut().find(|step| step.name == "Upload release receipt").ok_or("upload")?;
    let StepKind::Action { with, .. } = &mut upload.kind else { return Err("upload action".into()); };
    with.insert("name".to_owned(), "unbound-receipt".to_owned());
    assert!(!valid(&altered_upload, &rust), "receipt artifact name must bind run and attempt");

    let mut duplicate = spec.clone();
    let job = duplicate.jobs.get_mut("release-reconcile").ok_or("reconcile")?;
    let second = job.steps.iter().find(|step| step.name == "Release reconciliation-receipt-acquire").ok_or("acquire")?.clone();
    job.steps.push(second);
    assert!(!valid(&duplicate, &rust), "duplicate receipt acquisition must fail");

    let mut late_upload = spec.clone();
    let job = late_upload.jobs.get_mut("release-preflight").ok_or("preflight")?;
    let receipt = job.steps.iter().position(|step| step.name == "Release candidate-receipt").ok_or("receipt")?;
    let verify = job.steps.iter().position(|step| step.name == "Verify Rust channel manifest").ok_or("verify")?;
    job.steps.swap(receipt, verify);
    assert!(!valid(&late_upload, &rust), "receipt must follow verified Rust");
    Ok(())
}

#[test]
fn crate_release_uses_only_fixed_github_hosted_runner() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{ReleaseRenderContext, render_release_workflow};
    let (spec, rust) = fixture()?;
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };
    let workflow = render_release_workflow(&spec, &ctx, &rust)?;
    assert_eq!(workflow.matches("runs-on: ubuntu-24.04").count(), spec.jobs.len());
    for alternate in ["ubuntu-26.04", "self-hosted", "${{ inputs.runner }}"] {
        let mut altered = spec.clone();
        for job in altered.jobs.values_mut() {
            job.runs_on = alternate.to_owned();
        }
        let mut changed_ctx = ctx.clone();
        changed_ctx.runs_on = alternate.to_owned();
        assert!(render_release_workflow(&altered, &changed_ctx, &rust).is_err());
    }
    Ok(())
}


#[test]
fn rendered_workflow_has_one_hosted_environment_publisher() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};

    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
    };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    assert!(workflow.contains("release-publish:"));
    assert!(!workflow.contains("release-publish-bootstrap:"));
    assert!(!workflow.contains("secrets.CARGO_REGISTRY_TOKEN"));
    assert!(!workflow.contains("release-plz release"));
    assert!(workflow.contains("crate-release journal-start"));
    assert!(workflow.contains("crate-release journal-coordinate"));
    let publisher = spec.jobs.get("release-publish").ok_or("publisher")?;
    assert_eq!(publisher.runs_on, "ubuntu-24.04");
    assert_eq!(publisher.environment.as_deref(), Some("release"));
    assert_eq!(publisher.needs, ["release-preflight"]);
    assert_eq!(publisher.permissions.id_token, PermissionLevel::Write);
    assert_eq!(publisher.permissions.contents, PermissionLevel::Write);
    assert_eq!(publisher.condition.as_deref(), Some(publish_gate_condition("owner/project", "main", &spec.plan, true).as_str()));
    let position = |name: &str| publisher.steps.iter().position(|step| step.name == name);
    let source = position("Release source-require").ok_or("source admission")?;
    let verified = position("Release verify-admission").ok_or("verify admission")?;
    let acquire = position("Acquire release-plz").ok_or("acquire release-plz")?;
    let publish = position("Release journal-coordinate").ok_or("publish release")?;
    assert!(source < verified && verified < acquire && acquire < publish);
    assert!(publisher.steps.iter().find(|step| step.name == "Release journal-coordinate").is_some_and(|step| step.condition.as_deref() == Some(velnor_actions_workflow_renderer::release_gates::COORDINATE_CONDITION)));
    assert!(workflow.contains("id-token: write"));
    assert!(workflow.contains("needs: release-preflight") || workflow.contains("- release-preflight"));
    let mut no_oidc = spec.clone();
    no_oidc.jobs.get_mut("release-publish").ok_or("publisher")?.permissions.id_token = PermissionLevel::None;
    assert!(render_release_workflow(&no_oidc, &context, &rust).is_err());
    let mut bypass_gate = spec.clone();
    bypass_gate.jobs.get_mut("release-publish").ok_or("publisher")?.condition = None;
    assert!(render_release_workflow(&bypass_gate, &context, &rust).is_err());
    let mut two_publishers = spec.clone();
    let mut duplicate = two_publishers.jobs.get("release-publish").ok_or("publisher")?.clone();
    duplicate.role = velnor_actions_workflow_renderer::release_jobs::ReleaseRole::PublishBootstrap;
    two_publishers.jobs.insert("release-publish-bootstrap".to_owned(), duplicate);
    assert!(render_release_workflow(&two_publishers, &context, &rust).is_err());
    let mut self_hosted = spec.clone();
    self_hosted.jobs.get_mut("release-publish").ok_or("publisher")?.runs_on = "self-hosted".to_owned();
    assert!(render_release_workflow(&self_hosted, &context, &rust).is_err());
    let mut unprotected = spec.clone();
    unprotected.jobs.get_mut("release-publish").ok_or("publisher")?.environment = None;
    assert!(render_release_workflow(&unprotected, &context, &rust).is_err());
    let mut no_preflight = spec.clone();
    no_preflight.jobs.get_mut("release-publish").ok_or("publisher")?.needs.clear();
    assert!(render_release_workflow(&no_preflight, &context, &rust).is_err());
    Ok(())
}

#[test]
fn rendered_bootstrap_has_only_token_publisher() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};
    let (spec, rust) = fixture_for(ReleaseAuthentication::BootstrapToken)?;
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    assert!(workflow.contains("release-publish-bootstrap:"));
    assert!(!workflow.contains("release-publish:"));
    assert!(workflow.contains("secrets.CARGO_REGISTRY_TOKEN"));
    assert!(!workflow.contains("release-plz release"));
    assert!(workflow.contains("crate-release journal-start"));
    assert!(workflow.contains("crate-release journal-coordinate"));
    let publisher = spec.jobs.get("release-publish-bootstrap").ok_or("publisher")?;
    assert_eq!(publisher.environment.as_deref(), Some("release"));
    assert_eq!(publisher.needs, ["release-preflight"]);
    assert!(spec.jobs.values().all(|job| job.permissions.id_token == PermissionLevel::None));
    for (id, job) in &spec.jobs {
        for step in &job.steps {
            if let StepKind::Shell { env, .. } = &step.kind
                && env.values().any(|value| value.contains("secrets.CARGO_REGISTRY_TOKEN"))
            {
                assert_eq!(id, "release-publish-bootstrap");
                assert_eq!(step.name, "Release journal-coordinate");
            }
        }
    }
    Ok(())
}

#[test]
fn rendered_files_reject_divergent_release_intent() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{render_release_files, ReleaseRenderContext};
    let (spec, rust) = fixture()?;
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
    let config = ReleasePlzConfig { tag_pattern: "{{ package }}-v{{ version }}".into(), semver_check: true, packages: vec![ReleasePlzPackage { name: "example".into(), publish_features: vec![] }] };
    let intent = CrateReleaseIntent { schema: 1, repository: "owner/project".into(), default_branch: "main".into(), environment: "release".into(), required_workflow_path: ".github/workflows/ci.yml".into(), trusted_reviewer_id: 1, rust_version: rust, generator_version: env!("CARGO_PKG_VERSION").into(), generator_source_sha: "b".repeat(40), generator_manifest_sha256: "c".repeat(64), release_plz_version: "0.4.3".into(), runner: "ubuntu-24.04".into(), authentication: velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing, manifest_path: "Cargo.toml".into(), pr_write_paths: vec!["Cargo.toml".into()], registry: "crates-io".into(), packages: vec!["example".into()], publish_features: BTreeMap::from([("example".into(), vec![])]), new_crates: None, tag_template: config.tag_pattern.clone(), journal_predecessor: None, recovery: Some(velnor_actions_contract::config::CrateReleaseRecovery { package: "example".into(), version: "0.1.1".into(), source_sha: "a".repeat(40) }) };
    assert_release_pins(&spec, &ctx, &config, &intent);
    assert_predecessor_binding(&spec, &ctx, &config, &intent)?;
    let mut wrong_auth = intent.clone();
    wrong_auth.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    assert!(render_release_files(&spec, &ctx, &config, &wrong_auth).is_err());
    let (bootstrap_spec, _) = fixture_for(velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken)?;
    let bootstrap_files = render_release_files(&bootstrap_spec, &ctx, &config, &wrong_auth)?;
    assert!(!bootstrap_files.intent.bytes.contains("CARGO_REGISTRY_TOKEN"));
    let bootstrap_decoded: CrateReleaseIntent = toml::from_str(&bootstrap_files.intent.bytes)?;
    assert_eq!(bootstrap_decoded, wrong_auth);
    assert!(render_release_files(&bootstrap_spec, &ctx, &config, &intent).is_err());
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    assert_committed_workflow_lock(&files.workflow.bytes, &spec.concurrency.group)?;
    let stale = ".github/release-plz-publish.toml";
    assert!(velnor_actions_workflow_renderer::release_tree::release_stale_paths().contains(&stale));
    assert!(!files.as_sorted_vec().iter().any(|file| file.path == stale));
    assert_eq!(files.as_sorted_vec().len(), 3);
    let generated_paths: std::collections::BTreeSet<_> = files.as_sorted_vec().into_iter().map(|file| file.path).collect();
    let planned_paths: std::collections::BTreeSet<_> = velnor_actions_workflow_renderer::release_tree::RELEASE_CURRENT_PATHS.iter().map(|path| (*path).to_owned()).collect();
    assert_eq!(planned_paths, generated_paths);

    assert!(!files.intent.bytes.contains("CARGO_REGISTRY_TOKEN"));
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert_workspace_resume(&spec, &ctx, &config, &intent)?;
    assert_release_features(&spec, &ctx, &config, &intent)?;
    let mut wrong = intent.clone();
    wrong.registry = "alternate".into();
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = config.clone();
    wrong.tag_pattern = "{{package}}-v{{version}}".into();
    assert!(render_release_files(&spec, &ctx, &wrong, &intent).is_err());
    let mut wrong = intent.clone();
    wrong.recovery.as_mut().ok_or("recovery")?.version = "0.1.2".into();
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    for (key, value) in [("generator", "0.1.99"), ("plz", "0.4.4"), ("runner", "ubuntu-26.04")] {
        let mut wrong = intent.clone();
        match key {
            "generator" => wrong.generator_version = value.into(),
            "plz" => wrong.release_plz_version = value.into(),
            _ => wrong.runner = value.into(),
        }
        assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    }
    let mut wrong = intent.clone();
    wrong.repository = "owner/other".into();
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.environment = "other".into();
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.default_branch = "other".into();
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.packages = vec!["other".into()];
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.recovery.as_mut().ok_or("recovery")?.source_sha = "b".repeat(40);
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = config.clone();
    wrong.packages[0].name = "other".into();
    assert!(render_release_files(&spec, &ctx, &wrong, &intent).is_err());
    Ok(())
}

fn assert_committed_workflow_lock(workflow: &str, group: &str) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::create_dir_all(temp.path().join(".github/workflows"))?;
    let path = temp.path().join(".github/workflows/release.yml");
    std::fs::write(&path, workflow)?;
    for args in [vec!["init", "-q"], vec!["add", "."], vec!["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "workflow"]] {
        assert!(std::process::Command::new("git").current_dir(temp.path()).args(args).status()?.success());
    }
    let output = std::process::Command::new("git").current_dir(temp.path()).args(["rev-parse", "HEAD"]).output()?;
    assert!(output.status.success());
    let sha = std::str::from_utf8(&output.stdout)?.trim();
    std::fs::write(&path, workflow.replace("cancel-in-progress: false", "cancel-in-progress: true"))?;
    let committed = crate::crate_release_runtime::read_workflow_at(temp.path(), sha, &crate::crate_release_git::ReleaseGit::host_fixture())?;
    crate::crate_release_runtime::validate_workflow_concurrency(&committed, group)?;
    assert!(crate::crate_release_runtime::validate_workflow_concurrency(&std::fs::read(path)?, group).is_err());
    Ok(())
}

fn assert_workspace_resume(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_config::ReleasePlzPackage;
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let mut normal_spec = spec.clone();
    normal_spec.plan.version = None;
    normal_spec.plan.dispatch = false;
    normal_spec.triggers.dispatch_inputs.clear();
    normal_spec.jobs.get_mut("release-publish").ok_or("publisher")?.condition = Some(publish_gate_condition("owner/project", "main", &normal_spec.plan, true));
    let mut normal_intent = intent.clone();
    normal_intent.recovery = None;
    assert!(render_release_files(&normal_spec, ctx, config, &normal_intent).is_ok());
    let mut workspace_spec = normal_spec.clone();
    workspace_spec.plan.packages.insert("second".into(), "0.2.0".into());
    let mut workspace_intent = normal_intent.clone();
    workspace_intent.packages.push("second".into());
    workspace_intent.publish_features.insert("second".into(), vec![]);
    let mut workspace_config = config.clone();
    workspace_config.packages.push(ReleasePlzPackage { name: "second".into(), publish_features: vec![] });
    assert!(render_release_files(&workspace_spec, ctx, &workspace_config, &workspace_intent).is_ok());
    workspace_spec.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor { run_id: 42, run_attempt: 2, workflow_sha: "d".repeat(40) });
    workspace_intent.journal_predecessor = workspace_spec.journal_predecessor.clone();
    let resumed = render_release_files(&workspace_spec, ctx, &workspace_config, &workspace_intent)?;
    let decoded_workspace: CrateReleaseIntent = toml::from_str(&resumed.intent.bytes)?;
    assert_eq!(decoded_workspace.journal_predecessor, workspace_spec.journal_predecessor);

    Ok(())
}

fn assert_predecessor_binding(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let predecessor = velnor_actions_contract::config::CrateReleasePredecessor { run_id: 42, run_attempt: 2, workflow_sha: "d".repeat(40) };
    let mut resume_spec = spec.clone();
    let mut recovery_intent = intent.clone();
    resume_spec.journal_predecessor = Some(predecessor.clone());
    recovery_intent.journal_predecessor = Some(predecessor);
    let files = render_release_files(&resume_spec, ctx, config, &recovery_intent)?;
    let rendered: velnor_actions_contract::config::CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(rendered.journal_predecessor, recovery_intent.journal_predecessor);
    recovery_intent.journal_predecessor.as_mut().ok_or("predecessor")?.run_attempt += 1;
    assert!(render_release_files(&resume_spec, ctx, config, &recovery_intent).is_err());

    Ok(())
}

fn assert_release_pins(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) {
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    for field in ["commit", "digest"] {
        let mut wrong = intent.clone();
        if field == "commit" { wrong.generator_source_sha = "a".repeat(40); } else { wrong.generator_manifest_sha256 = "d".repeat(64); }
        assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    }
}

fn assert_release_features(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let mut feature_config = config.clone();
    feature_config.packages[0].publish_features = vec!["pty".into()];
    let mut feature_intent = intent.clone();
    feature_intent.publish_features.insert("example".into(), vec!["pty".into()]);
    let feature_files = render_release_files(spec, ctx, &feature_config, &feature_intent)?;
    let decoded_features: CrateReleaseIntent = toml::from_str(&feature_files.intent.bytes)?;
    assert_eq!(decoded_features.publish_features.get("example"), Some(&vec!["pty".into()]));
    assert!(feature_files.config.bytes.contains("publish_features = [\"pty\"]"));
    let mut wrong = intent.clone();
    wrong.publish_features.insert("example".into(), vec!["pty".into()]);
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    Ok(())
}

#[test]
fn release_tool_expectations_require_a_valid_consumer_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let raw = consumer_manifest(version);
    let acquire = |json: &str| crate::pins::consumer_acquire_step_with_manifest("ubuntu-24.04", version, Some(json));
    assert!(acquire(&raw).is_ok());
    let mut wrong: serde_json::Value = serde_json::from_str(&raw)?;
    wrong["version"] = "9.9.9".into();
    assert!(acquire(&serde_json::to_string(&wrong)?).is_err());
    let mut missing: serde_json::Value = serde_json::from_str(&raw)?;
    missing["targets"].as_array_mut().ok_or("targets")?.retain(|target| target["target"] != "x86_64-unknown-linux-gnu");
    assert!(acquire(&serde_json::to_string(&missing)?).is_err());
    let mut duplicate: serde_json::Value = serde_json::from_str(&raw)?;
    let targets = duplicate["targets"].as_array_mut().ok_or("targets")?;
    targets.push(targets[0].clone());
    assert!(acquire(&serde_json::to_string(&duplicate)?).is_err());
    let mut malformed: serde_json::Value = serde_json::from_str(&raw)?;
    malformed["targets"][0]["sha256"] = "not-a-digest".into();
    assert!(acquire(&serde_json::to_string(&malformed)?).is_err());
    Ok(())
}

#[test]
fn publishing_jobs_verify_generator_before_running_it() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::StepKind;
    use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
    for version in ["1..2", "1.2.3-rc.1", "latest"] {
        assert!(velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(version, "ubuntu-24.04", &"b".repeat(40), &"c".repeat(64)).is_err());
    }
    assert!(velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step("1.2.3", "ubuntu-26.04", &"b".repeat(40), &"c".repeat(64)).is_err());
    for authentication in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for(authentication)?;
        let workflow = render_release_workflow(&spec, &context, &rust)?;
        let mut tampered = spec.clone();
        let publisher = tampered.jobs.values_mut().find(|job| matches!(job.role, ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap)).ok_or("publisher")?;
        if let StepKind::Shell { env, .. } = &mut publisher.steps[0].kind { env.insert("VELNOR_GENERATOR_VERSION".into(), "0.1.99".into()); }
        assert!(render_release_workflow(&tampered, &context, &rust).is_err());
        for job in spec.jobs.values().filter(|job| matches!(job.role, ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap | ReleaseRole::Preflight)) {
            let acquire = job.steps.first().ok_or("missing acquisition")?;
            let StepKind::Shell { run, env } = &acquire.kind else { return Err("wrong acquisition kind".into()); };
            assert_eq!(env.get("VELNOR_GENERATOR_VERSION").map(String::as_str), Some(env!("CARGO_PKG_VERSION")));
            assert_eq!(env.get("GH_TOKEN").map(String::as_str), Some("${{ github.token }}"));
            assert!(run.iter().any(|part| part.contains("/usr/bin/base64 --decode")));
            assert!(workflow.contains("/usr/bin/base64 --decode"));
        }
    }
    Ok(())
}

#[test]
fn rendered_bootstrap_rejects_untrusted_generator_before_staging() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};
    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    let temp = tempfile::tempdir()?;
    let harness = temp.path().join("bootstrap_harness.py");
    std::fs::write(&harness, include_str!("release_steps_tests/bootstrap_harness.py"))?;
    let mut child = Command::new("python3").arg(harness).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().ok_or("missing stdin")?.write_all(workflow.as_bytes())?;
    let output = child.wait_with_output()?;
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    Ok(())
}

#[test]
fn publisher_runs_guarded_durable_coordinator_in_order() -> Result<(), Box<dyn std::error::Error>> {
    for auth in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for(auth)?;
        let job = spec.jobs.values().find(|job| job.display_name == "Release publish" || job.display_name == "Release bootstrap publish").ok_or("publisher")?;
        let names: Vec<_> = job.steps.iter().map(|step| step.name.as_str()).collect();
        let index = |name| names.iter().position(|candidate| *candidate == name).ok_or("missing step");
        assert!(index("Release source-require")? < index("Acquire cargo-semver-checks")?);
        assert!(index("Acquire cargo-semver-checks")? < index("Release receipt-acquire")?);
        assert!(index("Release verify-admission")? < index("Acquire release-plz")?);
        assert!(index("Acquire release-plz")? < index("Release journal-start")?);
        assert!(index("Release journal-start")? < index("Release journal-coordinate")?);
        assert!(!names.contains(&"Publish release"));
        assert!(!names.contains(&"Upload publication record"));
        let coordinate = &job.steps[index("Release journal-coordinate")?];
        let predicate = coordinate.condition.as_deref().ok_or("condition")?;
        for mode in ["publish", "repair", "mixed", "none"] {
            assert!(predicate.contains(&format!("release_mode == '{mode}'")));
        }
        assert!(valid(&spec, &rust));
        let mut tampered = spec.clone();
        let publisher = tampered.jobs.values_mut().find(|job| job.display_name == "Release publish" || job.display_name == "Release bootstrap publish").ok_or("publisher")?;
        let start = publisher.steps.iter().position(|step| step.name == "Release journal-start").ok_or("start")?;
        let coordinate = publisher.steps.iter().position(|step| step.name == "Release journal-coordinate").ok_or("coordinate")?;
        publisher.steps.swap(start, coordinate);
        assert!(!valid(&tampered, &rust));
    }
    Ok(())
}

#[test]
fn workspace_creation_dispatch_binds_source_and_each_package()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::{CrateReleaseIntent, NewCrateRelease};
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{render_release_files, ReleaseRenderContext};
    let (mut spec, rust) = fixture_for(ReleaseAuthentication::BootstrapToken)?;
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers.dispatch_inputs.retain(|input| input.name != "version");
    let creation = NewCrateRelease { source_sha: spec.plan.source_sha.clone(), packages: spec.plan.packages.clone() };
    spec.new_crates = Some(creation.clone());
    let release = RustReleaseConfig { enabled: true, registry: ReleaseRegistry::CratesIo,
        authentication: Some(ReleaseAuthentication::BootstrapToken), packages: vec!["example".into(), "second".into()],
        new_crates: Some(creation.clone()), required_workflow_path: ".github/workflows/ci.yml".into(), trusted_reviewer_id: 1, ..RustReleaseConfig::default() };
    let catalog = ToolCatalog::pinned().with_export_rustup_toolchain(false);
    let mise = MiseSetup { uses: format!("jdx/mise-action@{}", velnor_actions_actionlint::actions::MISE_ACTION_SHA), version: MISE_VERSION.to_owned(), sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned() };
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(env!("CARGO_PKG_VERSION"), "ubuntu-24.04", &"b".repeat(40), &"c".repeat(64))?;
    spec.jobs = assemble_jobs(&JobInputs { release: &release, gate: publish_gate_condition(&spec.repository, "main", &spec.plan, false), preparation_gate: "github.repository == 'owner/project' && github.event_name == 'push' && github.ref == 'refs/heads/main' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'".into(), registry: None, label: "ubuntu-24.04", mise: &mise, catalog: &catalog, acquire: &acquire, generator_version: env!("CARGO_PKG_VERSION") })?;
    let config = ReleasePlzConfig { tag_pattern: "{{ package }}-v{{ version }}".into(), semver_check: true, packages: spec.plan.packages.keys().map(|name| ReleasePlzPackage { name: name.clone(), publish_features: vec![] }).collect() };
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),"generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),"release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"bootstrap-token","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],"publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}","new_crates":creation,"recovery":null}))?;
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(files.workflow.bytes.contains("workflow_dispatch:"));
    assert!(files.workflow.bytes.contains("github.event.inputs.source_sha"));
    assert!(!files.workflow.bytes.contains("github.event.inputs.version"));
    let yaml: serde_json::Value = serde_saphyr::from_str(&files.workflow.bytes)?;
    let gate = yaml["jobs"]["release-publish-bootstrap"]["if"].as_str().ok_or("publisher gate")?;
    assert!(gate.contains("workflow_dispatch"));
    assert!(!gate.contains("event_name == 'push'"));
    let mut wrong = intent.clone();
    wrong.new_crates.as_mut().ok_or("creation")?.source_sha = "d".repeat(40);
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.new_crates.as_mut().ok_or("creation")?.packages.insert("second".into(), "0.2.1".into());
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    Ok(())
}

#[test]
fn bootstrap_handover_renders_normal_trusted_publisher() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{render_release_files, ReleaseRenderContext};
    let (mut spec, rust) = fixture()?;
    spec.plan.dispatch = false;
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers.dispatch_inputs.clear();
    spec.jobs.get_mut("release-publish").ok_or("publisher")?.condition =
        Some(publish_gate_condition("owner/project", "main", &spec.plan, true));
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: true,
        packages: ["example", "second"].into_iter().map(|name| ReleasePlzPackage {
            name: name.into(), publish_features: vec![],
        }).collect(),
    };
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({
        "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
        "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,
        "rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),
        "generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),
        "release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing",
        "manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],
        "publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}"
    }))?;
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on:"ubuntu-24.04".into() };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(decoded.new_crates.is_none());
    crate::crate_release_pr_acquisition::verify_reviewed_workflow(files.workflow.bytes.as_bytes(), &intent)?;
    crate::crate_release_pr_acquisition::integration_fixture::exercise(&intent, &files.intent.bytes, &files.workflow.bytes)?;
    assert_reviewed_rendered_workflow(&files.workflow.bytes, &intent)?;
    Ok(())
}

fn assert_reviewed_rendered_workflow(bytes: &str, intent: &velnor_actions_contract::config::CrateReleaseIntent) -> Result<(), Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = serde_saphyr::from_str(bytes)?;
    for key in ["env", "defaults", "permissions"] {
        let mut tampered = yaml.clone();
        tampered.as_object_mut().ok_or("workflow")?.insert(key.into(), serde_json::json!({"PATH": "/tmp/forged"}));
        assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&tampered)?.as_bytes(), intent).is_err(), "accepted global {key}");
    }
    for (path, value) in [
        ("/concurrency/group", serde_json::json!("forged")),
        ("/concurrency/cancel-in-progress", serde_json::json!(true)),
        ("/jobs/release-preflight/permissions/contents", serde_json::json!("write")),
        ("/jobs/release-preparation/outputs/changed", serde_json::json!("true")),
        ("/jobs/release-preparation/if", serde_json::json!("true")),
        ("/jobs/release-pr-create/permissions/id-token", serde_json::json!("write")),
        ("/jobs/release-pr-create/steps/1/run", serde_json::json!("echo forged")),
    ] {
        let mut tampered = yaml.clone();
        *tampered.pointer_mut(path).ok_or("workflow field")? = value;
        assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&tampered)?.as_bytes(), intent).is_err(), "accepted {path}");
    }
    let mut nested_intent = intent.clone();
    nested_intent.pr_write_paths = vec!["nested/workspace/Cargo.lock".into()];
    let mut nested_workflow = yaml.clone();
    nested_workflow["concurrency"]["group"] = serde_json::json!("release-crates-io-owner/project-bed46b222d05fcf033c7a100925229762295940199bb5c2e0aba841333bfa949");
    assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&nested_workflow)?.as_bytes(), &nested_intent).is_ok());
    nested_intent.pr_write_paths.clear();
    assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&nested_workflow)?.as_bytes(), &nested_intent).is_err());
    nested_intent.pr_write_paths = vec!["Cargo.lock".into(), "nested/workspace/Cargo.lock".into()];
    assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&nested_workflow)?.as_bytes(), &nested_intent).is_err());
    let prep = &yaml["jobs"]["release-preparation"];
    let steps = prep["steps"].as_array().ok_or("preparation steps")?;
    for id in ["release-pr-candidate", "release-pr-artifact"] {
        assert_eq!(steps.iter().filter(|step| step["id"].as_str() == Some(id)).count(), 1, "missing rendered {id}");
    }
    let preflight_steps = yaml["jobs"]["release-preflight"]["steps"].as_array().ok_or("preflight steps")?;
    assert_eq!(preflight_steps.iter().filter(|step| step["id"].as_str() == Some("release-receipt-upload")).count(), 1, "missing receipt upload id");
    assert_eq!(prep["outputs"]["artifact_id"].as_str(), Some("${{ steps.release-pr-artifact.outputs.artifact-id }}"));
    assert_eq!(prep["outputs"]["content_sha256"].as_str(), Some("${{ steps.release-pr-candidate.outputs.content_sha256 }}"));
    assert_eq!(yaml["jobs"]["release-pr-create"]["needs"][0].as_str(), Some("release-preparation"));
    assert_eq!(prep["outputs"]["artifact_digest"].as_str(), Some("${{ steps.release-pr-artifact.outputs.artifact-digest }}"));
    assert_eq!(prep["outputs"]["changed"].as_str(), Some("${{ steps.release-pr-candidate.outputs.changed }}"));
    let upload = steps.iter().find(|step| step["id"].as_str() == Some("release-pr-artifact")).ok_or("upload")?;
    assert_eq!(upload["if"].as_str(), Some("steps.release-pr-candidate.outputs.changed == 'true'"));
    let receipt = preflight_steps.iter().find(|step| step["id"].as_str() == Some("release-receipt-upload")).ok_or("receipt")?;
    assert_eq!(receipt["if"].as_str(), Some("steps.release-admission.outputs.eligible == 'true'"));
    let writer = &yaml["jobs"]["release-pr-create"];
    assert_eq!(writer["if"].as_str(), Some("needs.release-preparation.result == 'success' && needs.release-preparation.outputs.changed == 'true'"));
    let writer_steps = writer["steps"].as_array().ok_or("writer steps")?;
    let env = &writer_steps.last().ok_or("writer command")?["env"];
    for (name, output) in [("VELNOR_PR_ARTIFACT_ID", "artifact_id"), ("VELNOR_PR_ARTIFACT_DIGEST", "artifact_digest"), ("VELNOR_PR_CONTENT_SHA256", "content_sha256")] {
        assert_eq!(env[name].as_str(), Some(format!("${{{{ needs.release-preparation.outputs.{output} }}}}").as_str()));
    }
    let publisher = &yaml["jobs"]["release-publish"];
    assert!(publisher["if"].as_str().ok_or("gate")?.contains("github.event_name == 'push'"));
    assert_eq!(publisher["permissions"]["id-token"].as_str(), Some("write"));
    assert_eq!(publisher["environment"].as_str(), Some("release"));
    assert!(yaml["jobs"]["release-publish-bootstrap"].is_null());
    assert!(!bytes.contains("secrets.CARGO_REGISTRY_TOKEN"));
    Ok(())
}

#[test]
fn preparation_waits_for_negative_trusted_release_admission() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let preparation = spec.jobs.get("release-preparation").ok_or("preparation")?;
    assert_eq!(preparation.needs, ["release-preflight"]);
    let condition = preparation.condition.as_deref().ok_or("condition")?;
    assert!(condition.contains("needs.release-preflight.result == 'success'"));
    assert!(condition.contains("needs.release-preflight.outputs.eligible == 'false'"));
    for bad_condition in [
        condition.replace("needs.release-preflight.result == 'success'", "true"),
        condition.replace("needs.release-preflight.outputs.eligible == 'false'", "needs.release-preflight.outputs.eligible != 'true'"),
        condition.replace("needs.release-preflight.outputs.eligible == 'false'", "true"),
    ] {
        let mut forged = spec.clone();
        forged.jobs.get_mut("release-preparation").ok_or("preparation")?.condition = Some(bad_condition);
        assert!(!valid(&forged, &rust));
    }
    Ok(())
}

#[test]
fn crate_release_checkouts_use_attested_file_free_git() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;
    for authentication in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        for release_pr in [true, false] {
            let (spec, rust) = fixture_for_release_pr(authentication, release_pr)?;
            assert!(valid(&spec, &rust));
            for job in spec.jobs.values().filter(|job| job.role != ReleaseRole::PrCreate) {
                assert_eq!(job.steps[0].name, "Acquire Velnor");
                assert_eq!(job.steps[1].name, "Checkout");
                let StepKind::Shell { run, env } = &job.steps[1].kind else { return Err("policy checkout is not Velnor".into()); };
                assert_eq!(run[2], "checkout-policy");
                assert_eq!(env.get("GIT_TOKEN").map(String::as_str), Some("${{ secrets.GITHUB_TOKEN }}"));
                assert!(!job.steps.iter().any(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))));
                if job.role != ReleaseRole::Preparation {
                    let source = job.steps.iter().find(|step| step.name == "Checkout exact source").ok_or("source checkout")?;
                    let StepKind::Shell { run, env } = &source.kind else { return Err("source checkout is not Velnor".into()); };
                    assert_eq!(run[2], "checkout-source");
                    assert_eq!(env.get("VELNOR_SOURCE_SHA").map(String::as_str), Some("${{ steps.release-admission.outputs.source_sha }}"));
                    assert_eq!(env.get("GIT_TOKEN").map(String::as_str), Some("${{ secrets.GITHUB_TOKEN }}"));
                }
            }
            let mut forged = spec.clone();
            let step = &mut forged.jobs.get_mut("release-preflight").ok_or("preflight")?.steps[3];
            let StepKind::Shell { env, .. } = &mut step.kind else { return Err("source checkout is not Velnor".into()); };
            env.insert("VELNOR_SOURCE_SHA".into(), "${{ github.sha }}".into());
            assert!(!valid(&forged, &rust));
        }
    }
    Ok(())
}
