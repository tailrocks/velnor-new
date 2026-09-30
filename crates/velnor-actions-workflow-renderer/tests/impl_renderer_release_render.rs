//! Release workflow and file rendering cases.
//!
//! Proves the rendered `velnor-release.yml` shape (typed dispatch
//! inputs, per-job permissions, stable lock, no prohibited events)
//! and the three-file release tree with sorted paths.
use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::release_config::{
    BootstrapReleasePlzConfig, ReleasePlzConfig, ReleasePlzPackage,
};
use velnor_actions_workflow_renderer::release_jobs::{
    JobPermissions, ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseTriggers, publish_gate_condition,
};
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH, ReleaseRenderContext, release_stale_paths,
    render_release_files, render_release_workflow,
};
use velnor_actions_workflow_renderer::{RenderError, action_step, shell_step};

const REPOSITORY: &str = "acme/widgets";
const PLAN_ID: &str = "plan-1";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const LABEL: &str = "ubuntu-26.04";
const CHECKOUT: &str = "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683";
const VERSION: &str = "0.1.0";

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: PLAN_ID.to_owned(),
        repository: REPOSITORY.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates-io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
    }
}

fn triggers() -> ReleaseTriggers {
    let input = |name: &str, default: &str| DispatchInput {
        name: name.to_owned(),
        description: format!("Approved {name} reference"),
        required: true,
        default: Some(default.to_owned()),
    };
    ReleaseTriggers {
        push_branches: vec!["main".to_owned()],
        schedule: None,
        dispatch_inputs: vec![input("plan", PLAN_ID), input("source_sha", SHA)],
    }
}

fn checkout(exact: bool) -> Result<Step, RenderError> {
    let mut with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    if exact {
        with.insert("ref".to_owned(), SHA.to_owned());
    }
    action_step("Checkout", CHECKOUT, with)
}

fn publish_argv(config: &str) -> Vec<String> {
    [
        "mise",
        "exec",
        "--",
        "release-plz",
        "release",
        "--config",
        config,
    ]
    .iter()
    .map(ToString::to_string)
    .collect()
}

fn shell(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step(name, argv, env)
}

fn plain_shell(name: &str) -> Result<Step, RenderError> {
    shell(
        name,
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::new(),
    )
}

fn job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<String>,
    steps: Vec<Step>,
) -> ReleaseJobSpec {
    let environment = match role {
        ReleaseRole::PublishOidc => Some("crates-io".to_owned()),
        ReleaseRole::PublishBootstrap => Some("crates-io-bootstrap".to_owned()),
        _ => None,
    };
    ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        needs: needs.iter().map(ToString::to_string).collect(),
        condition,
        environment,
        permissions: JobPermissions::expected(role),
        steps,
    }
}

fn base_jobs(gate: &str) -> Result<BTreeMap<String, ReleaseJobSpec>, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "preparation".to_owned(),
        job(
            ReleaseRole::Preparation,
            &[],
            None,
            vec![checkout(false)?, plain_shell("Prepare PR")?],
        ),
    );
    jobs.insert(
        "preflight".to_owned(),
        job(
            ReleaseRole::Preflight,
            &["preparation"],
            None,
            vec![checkout(true)?, plain_shell("Validate")?],
        ),
    );
    jobs.insert(
        "publish".to_owned(),
        job(
            ReleaseRole::PublishOidc,
            &["preflight"],
            Some(gate.to_owned()),
            vec![
                checkout(true)?,
                shell(
                    "Publish",
                    publish_argv(RELEASE_CONFIG_PATH),
                    BTreeMap::new(),
                )?,
            ],
        ),
    );
    Ok(jobs)
}

fn spec(with_bootstrap: bool) -> Result<ReleaseWorkflowSpec, RenderError> {
    let gate = publish_gate_condition(REPOSITORY, &bootstrap());
    let mut jobs = base_jobs(&gate)?;
    if with_bootstrap {
        jobs.insert(
            "publish-bootstrap".to_owned(),
            job(
                ReleaseRole::PublishBootstrap,
                &["preflight"],
                Some(gate.clone()),
                vec![
                    checkout(true)?,
                    shell(
                        "Bootstrap publish",
                        publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
                        BTreeMap::from([(
                            "CARGO_REGISTRY_TOKEN".to_owned(),
                            "${{ secrets.CRATES_TOKEN }}".to_owned(),
                        )]),
                    )?,
                ],
            ),
        );
    }
    let reconcile_needs: Vec<&str> = if with_bootstrap {
        vec!["publish", "publish-bootstrap"]
    } else {
        vec!["publish"]
    };
    jobs.insert(
        "reconcile".to_owned(),
        job(
            ReleaseRole::Reconcile,
            &reconcile_needs,
            Some("always()".to_owned()),
            vec![checkout(false)?, plain_shell("Reconcile")?],
        ),
    );
    Ok(ReleaseWorkflowSpec {
        name: "Release".to_owned(),
        repository: REPOSITORY.to_owned(),
        triggers: triggers(),
        concurrency: stable_lock_group("crates-io", REPOSITORY, "root")?,
        jobs,
        bootstrap: bootstrap(),
        publish_environment: "crates-io".to_owned(),
        bootstrap_environment: "crates-io-bootstrap".to_owned(),
    })
}

fn context() -> ReleaseRenderContext {
    ReleaseRenderContext {
        generator_version: VERSION.to_owned(),
        runs_on: LABEL.to_owned(),
    }
}

#[test]
fn rendered_release_carries_permissions_lock_and_typed_dispatch() -> Result<(), RenderError> {
    let text = render_release_workflow(&spec(true)?, &context())?;
    for needle in [
        "type: string",
        "id-token: write",
        "environment: crates-io",
        "cancel-in-progress: false",
        "release-crates-io-acme/widgets-root",
        "github.repository ==",
        "persist-credentials",
    ] {
        assert!(text.contains(needle), "missing {needle}:\n{text}");
    }
    for forbidden in [
        "pull_request_target",
        "workflow_run",
        "__internal",
        "velnor-actions run",
    ] {
        assert!(!text.contains(forbidden), "leaked {forbidden}:\n{text}");
    }
    assert!(
        text.starts_with("# Generated by Velnor Actions 0.1.0;"),
        "{text}"
    );
    Ok(())
}

#[test]
fn render_rejects_label_mismatch() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").runs_on = "ubuntu-24.04".to_owned();
    let Err(error) = render_release_workflow(&bad, &context()) else {
        return Err(RenderError::InvalidWorkflow(
            "unexpectedly_valid".to_owned(),
        ));
    };
    assert!(error.to_string().contains("label_mismatch"), "{error}");
    Ok(())
}

#[test]
fn release_files_render_three_sorted_paths() -> Result<(), RenderError> {
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".to_owned(),
        semver_check: true,
        packages: vec![ReleasePlzPackage {
            name: "widgets".to_owned(),
            publish_features: vec!["pty".to_owned()],
        }],
    };
    let bootstrap = BootstrapReleasePlzConfig::new(config.clone())?;
    let files = render_release_files(&spec(false)?, &context(), &config, &bootstrap)?;
    let sorted = files.as_sorted_vec();
    let paths: Vec<&str> = sorted.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            ".github/velnor-release-plz-bootstrap.toml",
            ".github/velnor-release-plz.toml",
            ".github/workflows/velnor-release.yml",
        ]
    );
    assert!(files.config.bytes.contains("release_always = false"));
    assert!(
        files
            .bootstrap_config
            .bytes
            .contains("release_always = true")
    );
    assert_eq!(release_stale_paths().len(), 3);
    Ok(())
}
