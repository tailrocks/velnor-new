//! Policy gating and support-job construction.
//!
//! Velnor policy merges one typed job per repository validator plus
//! optional candidate jobs (P05-6: no Policy umbrella); consumer policy
//! merges exactly the shared policy lane (Alint) when configured and
//! rejects everything else. No toolchain-qualification job exists. The
//! always-on `actionlint` job arrives via typed IR and is emitted for
//! both policies; it is never support IR.

// Token-hygiene gate lives beside the policy gates (`#[path]`, no `lib.rs` edit).
mod guards;
mod tokens;
pub(crate) use guards::{check_candidate_invariants, check_final_gate, insert_support_job};

pub(crate) use tokens::check_token_hygiene;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_config::{
    RustPolicyConfig, ValidatorKind, VelnorSupportWorkflow, WorkflowPolicy,
};
use velnor_actions_contract_workflow::{Job, JobTimeout, Step, StepKind, StepRole};

use velnor_actions_workflow_steps::{
    ALINT_BINARY_VERSION, ALINT_USES, RenderError,
    steps::{self, upload_diagnostics_step},
};

use crate::{
    candidate::{candidate_job, release_job},
    context::{CANDIDATE_JOB_ID, FINAL_JOB_ID, RenderContext, ValidatorCommand},
};

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "actionlint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Actionlint";

/// Protected release job ID (Velnor policy, candidate mode only).
pub(crate) const RELEASE_JOB_ID: &str = "release";

/// Display name of the protected release job.
pub(crate) const RELEASE_DISPLAY_NAME: &str = "Release";

/// Release ref gate: the job runs only on protected refs (tags/branches).
pub(crate) const RELEASE_REF_CONDITION: &str = "github.ref_protected == true";

/// Policy-aware support merge: one typed job per validator plus candidates.
///
/// Velnor merges the repository validator set; consumers merge the
/// shared Alint lane when configured and `ConsumerV1` validators that have
/// explicit typed commands. Unselected validators, candidate validation,
/// and Velnor-only job IDs remain rejected. The lint job is base IR, not
/// support IR, so it passes through on both policies.
pub(crate) fn merge_support_jobs(
    jobs: &mut BTreeMap<String, Job>,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
    policy: WorkflowPolicy,
) -> Result<(), RenderError> {
    let policy_name = match policy {
        WorkflowPolicy::ConsumerV1 => "consumer-v1",
        WorkflowPolicy::VelnorRepositoryV1 => "velnor-repository-v1",
    };
    if policy == WorkflowPolicy::ConsumerV1 {
        reject_consumer_ir(jobs)?;
    }
    let Some(workflow) = support else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    for validator in &workflow.validators {
        if *validator == ValidatorKind::Actionlint {
            return Err(RenderError::PolicyRejected {
                policy: policy_name.to_owned(),
                problem: "actionlint_not_support".to_owned(),
            });
        }
        if !seen.insert(*validator) {
            return Err(RenderError::PolicyRejected {
                policy: policy_name.to_owned(),
                problem: "duplicate_validator".to_owned(),
            });
        }
    }
    if policy == WorkflowPolicy::ConsumerV1 {
        for validator in &workflow.validators {
            let selected_command = *validator == ValidatorKind::Zizmor
                && ctx
                    .validator_commands
                    .iter()
                    .any(|command| command.validator == *validator);
            if *validator != ValidatorKind::Alint && !selected_command {
                return Err(RenderError::PolicyRejected {
                    policy: policy_name.to_owned(),
                    problem: format!("forbidden_job:{}", validator.job_id()),
                });
            }
        }
        if workflow.candidate_validation {
            return Err(RenderError::PolicyRejected {
                policy: policy_name.to_owned(),
                problem: "candidate_requires_velnor_policy".to_owned(),
            });
        }
    }
    for validator in &workflow.validators {
        let job = match validator {
            ValidatorKind::Alint => alint_job(ctx)?,
            ValidatorKind::Actionlint => {
                return Err(RenderError::PolicyRejected {
                    policy: policy_name.to_owned(),
                    problem: "actionlint_not_support".to_owned(),
                });
            }
            ValidatorKind::CargoDeny | ValidatorKind::CargoMachete | ValidatorKind::Zizmor => {
                validator_job(ctx, *validator)?
            }
        };
        insert_support_job(jobs, validator.job_id(), job)?;
    }
    if workflow.candidate_validation {
        let Some(spec) = &ctx.candidate else {
            return Err(RenderError::PolicyRejected {
                policy: policy_name.to_owned(),
                problem: "candidate_without_spec".to_owned(),
            });
        };
        insert_support_job(jobs, CANDIDATE_JOB_ID, candidate_job(ctx, spec)?)?;
        insert_support_job(jobs, RELEASE_JOB_ID, release_job(ctx)?)?;
    }
    extend_final_needs(jobs, &workflow.validators, workflow.candidate_validation);
    Ok(())
}

/// Consumer policy: reject Velnor-only job IDs in base IR.
fn reject_consumer_ir(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for validator in ValidatorKind::repository_validators() {
        let id = validator.job_id();
        if jobs.contains_key(id) {
            return Err(RenderError::PolicyRejected {
                policy: "consumer-v1".to_owned(),
                problem: format!("forbidden_job:{id}"),
            });
        }
    }
    for id in [CANDIDATE_JOB_ID, RELEASE_JOB_ID] {
        if jobs.contains_key(id) {
            return Err(RenderError::PolicyRejected {
                policy: "consumer-v1".to_owned(),
                problem: format!("forbidden_job:{id}"),
            });
        }
    }
    Ok(())
}

/// Extend the final gate with merged support IDs (workflow-contract §4).
///
/// `Required` needs plan + every crate job + lint always, plus each merged
/// validator and candidate when those policy jobs exist. The release job
/// never gates: it publishes after merge on protected refs only.
fn extend_final_needs(
    jobs: &mut BTreeMap<String, Job>,
    validators: &[ValidatorKind],
    candidate: bool,
) {
    let mut extra = Vec::new();
    for validator in validators {
        let id = validator.job_id();
        if jobs.contains_key(id) {
            extra.push(id.to_owned());
        }
    }
    if candidate && jobs.contains_key(CANDIDATE_JOB_ID) {
        extra.push(CANDIDATE_JOB_ID.to_owned());
    }
    let Some(final_job) = jobs.get_mut(FINAL_JOB_ID) else {
        return;
    };
    for id in extra {
        if !final_job.needs.contains(&id) {
            final_job.needs.push(id);
        }
    }
}

/// SHA-256 of the `alint-v0.16.1-x86_64-unknown-linux-musl.tar.gz` asset.
///
/// Source: release-API `digest` plus fetched bytes; checked 2026-10-07.
/// Matches [`ALINT_BINARY_VERSION`]: the lane's CLI and the action's
/// binary are the same release. Freshness-inventory coverage for this
/// pin is a known gap (the inventory schema has no slot for it) — see
/// the policy rollout record.
const ALINT_SHA256_LINUX_X64: &str =
    "81ce8785e99a65bfa6a2e5cad6451dc3cac8a5cbc404b4452ae453ea392acb05";
/// SHA-256 of the `alint-v0.16.1-aarch64-apple-darwin.tar.gz` asset.
///
/// Source: release-API `digest` plus fetched bytes (binary executed:
/// `alint 0.16.1`); checked 2026-10-07. Same freshness gap as above.
const ALINT_SHA256_MACOS_ARM64: &str =
    "def639d9581520832096f0318b9f66204181679521b446da93a5d4875d5240e9";

/// Policy-prerequisite step: fetch, verify, and materialize the pinned policy.
///
/// The pin comes from `[stacks.rust.policy]` (version plus tarball
/// SHA-256, validated at config load); the lane verifies both
/// downloads, asserts the materialized package carries the mandatory
/// profile, and installs the policy helper plus the Alint CLI.
/// Anonymous downloads of public release assets (no API, no auth), so
/// the plain scrubbed shell fits: the step executes no repository
/// code and needs no ambient credentials. Runner assets selected from
/// `$RUNNER_OS-$RUNNER_ARCH` (no command substitution allowed in
/// emitted scripts).
fn policy_materialize_step(policy: &RustPolicyConfig) -> Result<Step, RenderError> {
    let version = &policy.version;
    let sha256 = &policy.sha256;
    let profile = policy.profile.file_name();
    let alint = ALINT_BINARY_VERSION
        .strip_prefix('v')
        .ok_or_else(|| RenderError::InvalidWorkflow("alint_version_shape_drift".to_owned()))?;
    let script = format!(
        "set -eu; case \"$RUNNER_OS-$RUNNER_ARCH\" in Linux-X64) POLICY_ASSET=\"linux-x86_64\"; ALINT_ASSET=\"x86_64-unknown-linux-musl\"; ALINT_SHA=\"{ALINT_SHA256_LINUX_X64}\";; macOS-ARM64) POLICY_ASSET=\"macos-arm64\"; ALINT_ASSET=\"aarch64-apple-darwin\"; ALINT_SHA=\"{ALINT_SHA256_MACOS_ARM64}\";; *) echo \"unsupported runner for rust policy\" >&2; exit 1;; esac; POLICY_TMP=\"$RUNNER_TEMP/velnor-policy-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT\"; rm -rf \"$POLICY_TMP\"; mkdir -p \"$POLICY_TMP\" .cache/rust-policy \"$HOME/.local/bin\"; curl -sSL -o \"$POLICY_TMP/policy.tar.gz\" \"https://github.com/tailrocks/rust-repository-policy/releases/download/v{version}/rust-repository-policy-{version}.tar.gz\"; printf \"%s  %s\\n\" \"{sha256}\" \"$POLICY_TMP/policy.tar.gz\" | sha256sum -c -; tar xzf \"$POLICY_TMP/policy.tar.gz\" -C .cache/rust-policy --strip-components=1; test -f \".cache/rust-policy/profiles/{profile}\"; cp \".cache/rust-policy/bin/$POLICY_ASSET/rust-repository-policy\" \"$HOME/.local/bin/rust-repository-policy\"; chmod +x \"$HOME/.local/bin/rust-repository-policy\"; curl -sSL -o \"$POLICY_TMP/alint.tar.gz\" \"https://github.com/asamarts/alint/releases/download/v{alint}/alint-v{alint}-$ALINT_ASSET.tar.gz\"; printf \"%s  %s\\n\" \"$ALINT_SHA\" \"$POLICY_TMP/alint.tar.gz\" | sha256sum -c -; tar xzf \"$POLICY_TMP/alint.tar.gz\" -C \"$POLICY_TMP\" --strip-components=1; cp \"$POLICY_TMP/alint\" \"$HOME/.local/bin/alint\"; chmod +x \"$HOME/.local/bin/alint\"; printf \"%s\\n\" \"$HOME/.local/bin\" >> \"$GITHUB_PATH\"; rm -rf \"$POLICY_TMP\""
    );
    steps::shell_step(
        "Materialize Rust policy",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )
}

/// Policy-lane diagnostics artifact name.
const POLICY_GAPS_ARTIFACT: &str = "policy-gaps";
/// Policy-lane diagnostics report file.
const POLICY_GAPS_REPORT: &str = "policy-gaps.txt";

/// Fixed policy-lane job: checkout, materialization, config validation,
/// the full-SHA Alint action (`check` with fail-on-warning), gap
/// diagnostics, and the diagnostics upload.
///
/// The job needs the `[stacks.rust.policy]` identity from the render
/// context and runs unconditionally: no job condition, no path
/// filters (triggers carry none by construction). Alint is the gate;
/// the diagnostics steps run `always()` so the `check-gaps` report
/// uploads even when the lane fails.
pub(crate) fn alint_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    let Some(policy) = &ctx.rust_policy else {
        return Err(RenderError::InvalidWorkflow(
            "alint_without_policy_identity".to_owned(),
        ));
    };
    let checkout = steps::checkout_step(&ctx.checkout_uses)?;
    let materialize = policy_materialize_step(policy)?;
    let validate = steps::shell_step(
        "Validate policy config",
        vec![
            "alint".to_owned(),
            "validate-config".to_owned(),
            "--config".to_owned(),
            ".alint.yml".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let mut with = BTreeMap::new();
    with.insert("path".to_owned(), ".".to_owned());
    with.insert("config".to_owned(), ".alint.yml".to_owned());
    with.insert("format".to_owned(), "github".to_owned());
    with.insert("fail-on-warning".to_owned(), "true".to_owned());
    with.insert("version".to_owned(), ALINT_BINARY_VERSION.to_owned());
    steps::scan_for_private_subcommands(ALINT_USES)?;
    let mut collect = steps::shell_step(
        "Collect policy gap diagnostics",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "set -eu; rust-repository-policy check-gaps --root . > policy-gaps.txt 2>&1 || true; test -f policy-gaps.txt"
                .to_owned(),
        ],
        BTreeMap::new(),
    )?;
    collect.condition = Some("always()".to_owned());
    let mut upload = upload_diagnostics_step(
        "Upload policy gap diagnostics",
        POLICY_GAPS_ARTIFACT,
        POLICY_GAPS_REPORT,
    )?;
    upload.condition = Some("always()".to_owned());
    Ok(Job {
        outputs: Vec::new(),
        display_name: ValidatorKind::Alint.display_name().to_owned(),
        runs_on: ctx.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checkout,
            materialize,
            validate,
            Step {
                name: "Run Alint".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Action {
                    uses: ALINT_USES.to_owned(),
                    with,
                    env: BTreeMap::new(),
                },
            },
            collect,
            upload,
        ],
    })
}

/// Fixed validator job: checkout plus its caller-supplied command.
///
/// The validator runs its pinned analyzer with ambient auth: the cold
/// tool bootstrap needs authenticated quota (the scrub overlay broke
/// `ubi:` installs with API 401s and zizmor with empty-token aborts,
/// CI run 36815180228). Static analyzers execute no repository code;
/// cargo-backed validators (deny) run from the cargo isolation dir
/// with an absolute manifest path, so repo `.cargo/config.toml`
/// providers never execute while ambient auth is in scope. The step
/// carries no scrub keys at all.
pub(crate) fn validator_job(
    ctx: &RenderContext,
    validator: ValidatorKind,
) -> Result<Job, RenderError> {
    let command = find_validator_command(&ctx.validator_commands, validator)?;
    let mut step = steps::ambient_shell_step(&command.name, command.argv.clone(), BTreeMap::new())?;
    step.role = Some(match validator {
        ValidatorKind::CargoDeny => StepRole::CargoDeny,
        ValidatorKind::CargoMachete => StepRole::CargoMachete,
        ValidatorKind::Zizmor => StepRole::Zizmor,
        ValidatorKind::Alint | ValidatorKind::Actionlint => {
            return Err(RenderError::InvalidWorkflow(format!(
                "validator_not_shell:{}",
                validator.job_id()
            )));
        }
    });
    Ok(Job {
        outputs: Vec::new(),
        display_name: validator.display_name().to_owned(),
        runs_on: ctx.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![steps::checkout_step(&ctx.checkout_uses)?, step],
    })
}

/// Exactly one command per validator; missing or duplicated fails closed.
fn find_validator_command(
    commands: &[ValidatorCommand],
    validator: ValidatorKind,
) -> Result<&ValidatorCommand, RenderError> {
    let mut found = None;
    for command in commands {
        if command.validator == validator {
            if found.is_some() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "duplicate_validator_command:{}",
                    validator.job_id()
                )));
            }
            found = Some(command);
        }
    }
    found.ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("validator_without_command:{}", validator.job_id()))
    })
}
