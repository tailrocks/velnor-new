//! Release workflow rendering, file assembly, and determinism cases.
use std::collections::BTreeMap;
use velnor_actions_contract::ScheduleTrigger;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_config::{
    BootstrapReleasePlzConfig, ReleasePlzConfig, ReleasePlzPackage,
};
use velnor_actions_workflow_renderer::release_jobs::{
    ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, publish_gate_condition,
};
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH, RELEASE_TREE_PATHS, RELEASE_WORKFLOW_PATH,
    ReleaseRenderContext, release_stale_paths, render_release_files, render_release_workflow,
};
use velnor_actions_workflow_renderer::{action_step, shell_step};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const REPO: &str = "acme/widgets";
const LABEL: &str = "ubuntu-24.04";
const ENV: &str = "crates-io";
const VERSION: &str = "0.1.0";

fn checkout_uses() -> String {
    format!("actions/checkout@{:040x}", 0)
}

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: "plan-1".to_owned(),
        repository: REPO.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates_io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
    }
}

pub(crate) fn ctx() -> ReleaseRenderContext {
    ReleaseRenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
    }
}

fn config() -> ReleasePlzConfig {
    ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".to_owned(),
        semver_check: true,
        packages: vec![ReleasePlzPackage {
            name: "widgets".to_owned(),
            publish_features: Vec::new(),
        }],
    }
}

fn exact_checkout() -> Result<velnor_actions_contract::Step, RenderError> {
    action_step(
        "Checkout",
        &checkout_uses(),
        BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("ref".to_owned(), SHA.to_owned()),
        ]),
    )
}

fn plain_job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<&str>,
    steps: Vec<velnor_actions_contract::Step>,
) -> ReleaseJobSpec {
    ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: condition.map(str::to_owned),
        environment: None,
        permissions: JobPermissions::expected(role),
        steps,
    }
}

fn publish_job(
    role: ReleaseRole,
    needs: &[&str],
    config_path: &str,
    env: &[(&str, &str)],
) -> Result<ReleaseJobSpec, RenderError> {
    let gate = publish_gate_condition(REPO, &bootstrap());
    let checkout = exact_checkout()?;
    let publish = shell_step(
        "Publish",
        ["release-plz", "release", "--config", config_path]
            .iter()
            .map(ToString::to_string)
            .collect(),
        env.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )?;
    Ok(ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: Some(gate),
        environment: Some(ENV.to_owned()),
        permissions: JobPermissions::expected(role),
        steps: vec![checkout, publish],
    })
}

pub(crate) fn spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    let checkout = exact_checkout()?;
    let run = shell_step(
        "Run",
        vec!["echo".to_owned(), "ok".to_owned()],
        BTreeMap::new(),
    )?;
    let jobs = BTreeMap::from([
        (
            "release-preparation".to_owned(),
            plain_job(ReleaseRole::Preparation, &[], None, vec![run.clone()]),
        ),
        (
            "release-preflight".to_owned(),
            plain_job(
                ReleaseRole::Preflight,
                &["release-preparation"],
                None,
                vec![checkout, run.clone()],
            ),
        ),
        (
            "release-publish".to_owned(),
            publish_job(
                ReleaseRole::PublishOidc,
                &["release-preflight"],
                RELEASE_CONFIG_PATH,
                &[],
            )?,
        ),
        (
            "release-reconcile".to_owned(),
            plain_job(
                ReleaseRole::Reconcile,
                &["release-publish"],
                Some("always()"),
                vec![run],
            ),
        ),
    ]);
    Ok(ReleaseWorkflowSpec {
        name: "Velnor Release".to_owned(),
        repository: REPO.to_owned(),
        triggers: ReleaseTriggers {
            push_branches: vec!["main".to_owned()],
            schedule: Some(ScheduleTrigger {
                cron: vec!["0 6 * * 1".to_owned()],
            }),
            dispatch_inputs: vec![
                DispatchInput {
                    name: "plan".to_owned(),
                    description: "approved plan".to_owned(),
                    required: true,
                    default: Some("plan-1".to_owned()),
                },
                DispatchInput {
                    name: "source_sha".to_owned(),
                    description: "approved source".to_owned(),
                    required: true,
                    default: Some(SHA.to_owned()),
                },
            ],
        },
        concurrency: ReleaseConcurrency {
            group: "release-acme/widgets".to_owned(),
            cancel_in_progress: false,
        },
        jobs,
        bootstrap: bootstrap(),
        publish_environment: ENV.to_owned(),
        bootstrap_environment: "crates-io-bootstrap".to_owned(),
    })
}

#[test]
fn render_emits_marker_triggers_and_pinned_jobs() -> Result<(), RenderError> {
    let text = render_release_workflow(&spec()?, &ctx())?;
    let first = text.lines().next().expect("marker line");
    assert_eq!(
        first,
        "# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate."
    );
    for needle in [
        "name: Velnor Release",
        "runs-on: ubuntu-24.04",
        "branches",
        "main",
        "cron: 0 6 * * 1",
        "workflow_dispatch",
        "plan-1",
        SHA,
        "environment: crates-io",
        "id-token: write",
        "cancel-in-progress: false",
        "release-acme/widgets",
        "needs:",
        "always()",
        "persist-credentials",
        "release-plz",
        RELEASE_CONFIG_PATH,
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
    for forbidden in [
        "pull_request",
        "workflow_run",
        "pull_request_target",
        "secrets.",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(!text.contains(forbidden), "leaked {forbidden}");
    }
    Ok(())
}

#[test]
fn render_rejects_label_mismatch_and_bad_context() -> Result<(), RenderError> {
    let mut skewed = spec()?;
    skewed
        .jobs
        .get_mut("release-preflight")
        .expect("job")
        .runs_on = "ubuntu-22.04".to_owned();
    assert!(matches!(
        render_release_workflow(&skewed, &ctx()),
        Err(RenderError::InvalidWorkflow(text)) if text == "label_mismatch:release-preflight"
    ));
    let bad_version = ReleaseRenderContext {
        generator_version: "bad version!".to_owned(),
        runs_on: LABEL.to_owned(),
    };
    assert!(matches!(
        render_release_workflow(&spec()?, &bad_version),
        Err(RenderError::BadVersion(_))
    ));
    let bad_label = ReleaseRenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: "ubuntu-latest".to_owned(),
    };
    assert!(render_release_workflow(&spec()?, &bad_label).is_err());
    Ok(())
}

#[test]
fn files_carry_fixed_paths_sorted() -> Result<(), RenderError> {
    let wrapped = BootstrapReleasePlzConfig::new(config())?;
    let files = render_release_files(&spec()?, &ctx(), &config(), &wrapped)?;
    assert_eq!(files.workflow.path, RELEASE_WORKFLOW_PATH);
    assert_eq!(files.config.path, RELEASE_CONFIG_PATH);
    assert_eq!(files.bootstrap_config.path, RELEASE_BOOTSTRAP_CONFIG_PATH);
    let sorted = files.as_sorted_vec();
    let paths: Vec<&str> = sorted.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            RELEASE_BOOTSTRAP_CONFIG_PATH,
            RELEASE_CONFIG_PATH,
            RELEASE_WORKFLOW_PATH
        ]
    );
    assert!(files.config.bytes.contains("release_always = false"));
    assert!(
        files
            .bootstrap_config
            .bytes
            .contains("release_always = true")
    );
    Ok(())
}

#[test]
fn stale_paths_cover_the_release_family_for_cleanup() {
    assert_eq!(
        release_stale_paths(),
        &[
            RELEASE_BOOTSTRAP_CONFIG_PATH,
            RELEASE_CONFIG_PATH,
            RELEASE_WORKFLOW_PATH,
        ]
    );
    let mut sorted = release_stale_paths().to_vec();
    sorted.sort_unstable();
    assert_eq!(release_stale_paths(), sorted.as_slice(), "sorted");
    assert_eq!(RELEASE_TREE_PATHS, release_stale_paths());
}

#[test]
fn rendering_is_deterministic_across_runs() -> Result<(), RenderError> {
    let wrapped = BootstrapReleasePlzConfig::new(config())?;
    let first = render_release_files(&spec()?, &ctx(), &config(), &wrapped)?;
    let wrapped = BootstrapReleasePlzConfig::new(config())?;
    let second = render_release_files(&spec()?, &ctx(), &config(), &wrapped)?;
    assert_eq!(first, second, "byte-identical files");
    assert_eq!(
        render_release_workflow(&spec()?, &ctx())?,
        render_release_workflow(&spec()?, &ctx())?,
        "byte-identical workflow"
    );
    Ok(())
}
