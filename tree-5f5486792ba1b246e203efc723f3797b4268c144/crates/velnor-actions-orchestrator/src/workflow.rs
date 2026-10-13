//! Workflow-IR, render-context, and actionlint-input construction.

use std::collections::BTreeMap;

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy};
use velnor_actions_contract::{
    Concurrency, GeneratorLock, GeneratorValidation, Permissions, ReleaseManifest, Step, Trigger,
    VelnorConfig, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy, target_for_runner_label,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::TaskGroup;
use velnor_actions_workflow_renderer::render::{
    CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, FINAL_JOB_ID, PLAN_JOB_ID,
    PolicyCommand, RenderContext, TASK_JOB_ID, WORKFLOW_PATH,
};
use velnor_actions_workflow_renderer::steps::{
    ASSET_SHA_ENV, ASSET_URL_ENV, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX, acquire_velnor_step,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::vectors::{candidate_spec, verify_tools_argv};
use crate::workflow_jobs::{final_job, lint_job, plan_job, task_job};

pub(crate) use crate::workflow_jobs::LINT_JOB_ID;

/// Pinned `actions/checkout` ref (cli-contract section 4 sample pin).
pub const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

/// Default literal runner label when the config omits the override.
pub const DEFAULT_RUNNER_LABEL: &str = "ubuntu-26.04";

/// Fixed request directory rendered for internal plan/merge steps.
pub(crate) const REQUEST_DIR: &str = "$RUNNER_TEMP/velnor/request";

/// Complete renderer input derived from one discovery.
#[derive(Debug, Clone)]
pub struct WorkflowPlan {
    /// Stack-neutral workflow IR.
    pub ir: WorkflowIr,
    /// Support jobs for the Velnor policy, none for consumers.
    pub support: Option<VelnorSupportWorkflow>,
    /// Validated renderer scalars.
    pub context: RenderContext,
    /// Actionlint config input.
    pub actionlint: ActionlintConfigInput,
}

/// Build renderer input from config, branch, label, and discovery.
///
/// # Errors
///
/// Returns contract, render-context, or tool-request errors.
pub(crate) fn build_workflow(
    config: &VelnorConfig,
    branch: &str,
    label: &str,
    discovery: &Discovery,
) -> Result<WorkflowPlan, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let policy = config.workflow.policy;
    let support = match policy {
        WorkflowPolicy::ConsumerV1 => None,
        WorkflowPolicy::VelnorRepositoryV1 => {
            Some(policy.support_workflow(config.workflow.generator_validation))
        }
    };
    let mut jobs = BTreeMap::new();
    let acquire = match policy {
        WorkflowPolicy::ConsumerV1 => Some(consumer_acquire_step(label, &version, discovery)?),
        WorkflowPolicy::VelnorRepositoryV1 => None,
    };
    jobs.insert(PLAN_JOB_ID.to_owned(), plan_job(label, acquire));
    let task_groups: Vec<&TaskGroup> = discovery
        .task_groups
        .iter()
        .filter(|group| !group.no_test_targets)
        .collect();
    if !task_groups.is_empty() {
        jobs.insert(
            TASK_JOB_ID.to_owned(),
            task_job(label, &task_groups, &catalog)?,
        );
    }
    jobs.insert(LINT_JOB_ID.to_owned(), lint_job(label, &catalog)?);
    jobs.insert(
        FINAL_JOB_ID.to_owned(),
        final_job(label, !task_groups.is_empty()),
    );
    let ir = WorkflowIr {
        name: config.workflow.name.clone(),
        triggers: Trigger {
            pull_request_types: EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect(),
            push_branches: vec![branch.to_owned()],
            merge_group: true,
        },
        permissions: Permissions {
            contents: "read".to_owned(),
            actions: "read".to_owned(),
        },
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    };
    let context = render_context(config, label, &version, &catalog)?;
    let actionlint = actionlint_input(policy, &version);
    Ok(WorkflowPlan {
        ir,
        support,
        context,
        actionlint,
    })
}

/// Attach a lock-backed Acquire step to the plan job (Velnor policy only).
///
/// Reads the runner-target record from an already-verified lock, stages the
/// digest-verified binary under `$RUNNER_TEMP`, and inserts the step between
/// checkout and plan. Consumer generation never calls this: it embeds the
/// release manifest instead, and never reads the lock.
pub(crate) fn attach_lock_acquire(
    ir: &mut WorkflowIr,
    lock: &GeneratorLock,
    label: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    let target = target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
        problem: format!("unsupported_target_for_runner:{label}"),
    })?;
    let record = lock
        .binary_for_target(target)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("lock_missing_target:{target}"),
        })?;
    let Some(plan) = ir.jobs.get_mut(PLAN_JOB_ID) else {
        return Err(OrchestratorError::Contract {
            problem: "plan_job_missing".to_owned(),
        });
    };
    let staged = format!("{STAGED_BINARY_PREFIX}{version}");
    let step = acquire_step(&record.artifact, &record.sha256, &staged)?;
    plan.steps.insert(1, step);
    Ok(())
}

/// Consumer Acquire step from the release manifest of this exact version.
///
/// Bootstrap contract §2: a source build (no embedded manifest) fails
/// consumer generation with a provenance diagnostic recommending an
/// official release; it never emits an unverified URL or placeholder digest.
fn consumer_acquire_step(
    label: &str,
    version: &str,
    discovery: &Discovery,
) -> Result<Step, OrchestratorError> {
    consumer_acquire_from(label, version, release_manifest_json(discovery).as_deref())
}

/// Consumer Acquire step from an explicit manifest (pure; `None` fails).
fn consumer_acquire_from(
    label: &str,
    version: &str,
    json: Option<&str>,
) -> Result<Step, OrchestratorError> {
    let Some(json) = json else {
        return Err(OrchestratorError::Contract {
            problem: "consumer_requires_release_install:install an official velnor-actions release"
                .to_owned(),
        });
    };
    let manifest = ReleaseManifest::parse_json(json, "release-manifest.json")?;
    manifest.validate("release-manifest.json")?;
    if manifest.version != version {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "release_manifest_version_mismatch:{}:{version}",
                manifest.version
            ),
        });
    }
    let target = target_for_runner_label(label).ok_or_else(|| OrchestratorError::Contract {
        problem: format!("unsupported_target_for_runner:{label}"),
    })?;
    let record = manifest
        .record_for_target(target)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("manifest_missing_target:{target}"),
        })?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &format!("{STAGED_BINARY_PREFIX}{version}"),
    )
}

/// Embedded release-manifest JSON: compile-time release provenance.
///
/// Baked `VELNOR_RELEASE_MANIFEST_JSON` wins; otherwise the debug-only
/// discovery fixture applies. Release builds have no fixture path, so a
/// source build always fails the consumer gate.
fn release_manifest_json(discovery: &Discovery) -> Option<String> {
    if let Some(baked) = option_env!("VELNOR_RELEASE_MANIFEST_JSON") {
        return Some(baked.to_owned());
    }
    discovery.consumer_manifest_json.clone()
}

/// `cfg(test)`-only fixture manifest matching the workspace version.
#[cfg(test)]
fn test_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"]
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://example.invalid/releases/download/{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{targets}]}}"
    )
}

/// Digest-verified staging step: fetch URL, check SHA-256, make executable.
fn acquire_step(url: &str, sha: &str, staged: &str) -> Result<Step, OrchestratorError> {
    let dir = staged.rsplit_once('/').map_or(staged, |(head, _)| head);
    let script = format!(
        "mkdir -p {dir} && curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
    );
    let env = BTreeMap::from([
        (ASSET_URL_ENV.to_owned(), url.to_owned()),
        (ASSET_SHA_ENV.to_owned(), sha.to_owned()),
    ]);
    Ok(acquire_velnor_step(
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?)
}

/// Renderer scalars: version, label, staged path, request dir, pins.
fn render_context(
    config: &VelnorConfig,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let policy_commands = if velnor {
        vec![PolicyCommand {
            name: "Verify pinned tools".to_owned(),
            argv: verify_tools_argv(catalog)?,
        }]
    } else {
        Vec::new()
    };
    let candidate =
        if velnor && config.workflow.generator_validation == GeneratorValidation::Candidate {
            Some(candidate_spec(catalog)?)
        } else {
            None
        };
    Ok(RenderContext {
        generator_version: version.to_owned(),
        runs_on: label.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{version}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        policy_commands,
        candidate,
    })
}

/// Actionlint input: generated workflow path plus policy-graded ignores.
fn actionlint_input(policy: WorkflowPolicy, version: &str) -> ActionlintConfigInput {
    let mut input = ActionlintConfigInput::new(version).with_workflow_path(WORKFLOW_PATH);
    input.policy = match policy {
        WorkflowPolicy::ConsumerV1 => IgnorePolicy::Consumer,
        WorkflowPolicy::VelnorRepositoryV1 => IgnorePolicy::VelnorProtected,
    };
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_build_consumer_generation_fails_with_provenance() {
        let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", None);
        assert!(err.is_err_and(|err| {
            err.to_string()
                .contains("consumer_requires_release_install")
        }));
    }

    #[test]
    fn consumer_manifest_mismatch_and_bad_target_fail() {
        let err = consumer_acquire_from("ubuntu-26.04", "9.9.9", Some(&test_manifest_json()));
        assert!(err.is_err_and(|err| err.to_string().contains("version_mismatch")));
        let err = consumer_acquire_from("ubuntu-26.04-arm", "0.1.0", Some(&test_manifest_json()));
        assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
        let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", Some("not json"));
        assert!(err.is_err());
    }

    #[test]
    fn fixture_manifest_embeds_runner_target_record() {
        let step = consumer_acquire_from(
            "ubuntu-26.04",
            env!("CARGO_PKG_VERSION"),
            Some(&test_manifest_json()),
        )
        .map(|step| step.name);
        assert_eq!(
            step.map_err(|err| err.to_string()),
            Ok("Acquire Velnor".to_owned())
        );
    }

    #[test]
    fn lock_acquire_inserts_digest_verified_stage() {
        use velnor_actions_contract::{
            Concurrency, GeneratorBinary, LockedGenerator, MiseBootstrap, Permissions, Trigger,
        };
        let lock = GeneratorLock {
            schema: 1,
            generator: LockedGenerator {
                binary: "velnor-actions".to_owned(),
                version: "0.1.0".to_owned(),
                binaries: vec![GeneratorBinary {
                    target: "x86_64-unknown-linux-gnu".to_owned(),
                    artifact: "https://example.invalid/r".to_owned(),
                    sha256: "a".repeat(64),
                }],
            },
            mise_bootstrap: MiseBootstrap {
                version: "2026.9.16".to_owned(),
                artifact: "https://example.invalid/m".to_owned(),
                sha256: "b".repeat(64),
            },
            actions: Vec::new(),
        };
        let mut ir = WorkflowIr {
            name: "CI".to_owned(),
            triggers: Trigger {
                pull_request_types: Vec::new(),
                push_branches: Vec::new(),
                merge_group: false,
            },
            permissions: Permissions {
                contents: "read".to_owned(),
                actions: "read".to_owned(),
            },
            concurrency: Concurrency {
                group: "g".to_owned(),
                cancel_in_progress: "c".to_owned(),
            },
            jobs: BTreeMap::from([("velnor-plan".to_owned(), plan_job("ubuntu-26.04", None))]),
        };
        assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").is_ok());
        let names: Vec<&str> = ir.jobs["velnor-plan"]
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Checkout", "Acquire Velnor", "Plan"]);
        assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04-arm", "0.1.0").is_err());
    }
}
