//! Workflow-IR documents and the exact two-file generated tree.
//!
//! Fail-closed gates: exact triggers, concurrency, single runner label,
//! consumer support rejection, and candidate-never-plans invariants.

use velnor_actions_contract::{
    Concurrency, Trigger, VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};

use crate::{
    RenderError, commands, document, guard, marker, steps, support,
    yaml::{Yaml, render_yaml},
};

/// Generated workflow path inside the repository.
pub const WORKFLOW_PATH: &str = ".github/workflows/velnor.yml";
/// Generated actionlint config path inside the repository.
pub const ACTIONLINT_PATH: &str = ".github/actionlint.yaml";
/// Exact pull-request event types.
pub const EXPECTED_PR_TYPES: &[&str] = &["opened", "synchronize", "reopened", "ready_for_review"];
/// Exact concurrency group expression.
pub const CONCURRENCY_GROUP: &str =
    "velnor-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}";
/// Exact cancel-in-progress expression (PR events only).
pub const CONCURRENCY_CANCEL: &str = "${{ github.event_name == 'pull_request' }}";
/// Final gate job ID.
pub const FINAL_JOB_ID: &str = "velnor-final";
/// Exact required-check display name.
pub const FINAL_DISPLAY_NAME: &str = "Velnor / Required";
/// Final gate condition.
pub const FINAL_CONDITION: &str = "always()";
/// Planner job ID: the sole matrix producer.
pub const PLAN_JOB_ID: &str = "velnor-plan";
/// Matrix consumer job ID.
pub const TASK_JOB_ID: &str = "velnor-task";
/// Candidate validation job ID (Velnor policy only).
pub const CANDIDATE_JOB_ID: &str = "velnor-candidate";
/// Repository-structure lint job ID (Velnor policy only).
pub const ALINT_JOB_ID: &str = "velnor-alint";
/// Dependency/security policy job ID (Velnor policy only).
pub const POLICY_JOB_ID: &str = "velnor-policy";
/// Sole full-SHA exception: pinned Alint tag for `velnor-alint` only.
pub const ALINT_USES: &str = "asamarts/alint@v0.16.1";

/// Caller-supplied validated scalars the IR cannot carry.
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// Exact generator version for the marker and staged path.
    pub generator_version: String,
    /// Single literal versioned Ubuntu label every job must use.
    pub runs_on: String,
    /// Digest-verified staged binary under runner temp.
    pub staged_binary: String,
    /// Internal request directory under runner temp.
    pub request_dir: String,
    /// Pinned `actions/checkout` ref for rendered support jobs.
    pub checkout_uses: String,
    /// Fixed shell steps for the `velnor-policy` job.
    pub policy_commands: Vec<PolicyCommand>,
    /// Fixed vectors for the `velnor-candidate` job, when enabled.
    pub candidate: Option<CandidateSpec>,
}

/// One fixed policy-job shell step: display name plus validated argv.
#[derive(Debug, Clone)]
pub struct PolicyCommand {
    /// Step display name.
    pub name: String,
    /// Fixed argument vector.
    pub argv: Vec<String>,
}

/// Fixed candidate-job vectors (Velnor policy only).
#[derive(Debug, Clone)]
pub struct CandidateSpec {
    /// Fixed candidate-build argv.
    pub build: Vec<String>,
    /// Fixed candidate-qualification argv.
    pub qualify: Vec<String>,
}

/// One rendered file: repository-relative path plus bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    /// Repository-relative output path.
    pub path: String,
    /// Complete file bytes including the marker.
    pub bytes: String,
}

/// Exactly the two generated files, sorted by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTree {
    /// The two files: actionlint config plus workflow.
    pub files: Vec<RenderedFile>,
}

impl RenderedTree {
    /// Fetch file bytes by repository-relative path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.bytes.as_str())
    }
}

impl RenderContext {
    /// Validate every context scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        validate_runs_on(&self.runs_on)?;
        validate_staged_binary(&self.staged_binary, &self.generator_version)?;
        validate_request_dir(&self.request_dir)?;
        steps::checkout_step(&self.checkout_uses).map(|_| ())?;
        for command in &self.policy_commands {
            if command.name.trim().is_empty() {
                return Err(RenderError::BadCommand("empty_policy_name".to_owned()));
            }
            commands::validate_command_argv(&command.argv)?;
        }
        if let Some(candidate) = &self.candidate {
            commands::validate_command_argv(&candidate.build)?;
            commands::validate_command_argv(&candidate.qualify)?;
        }
        Ok(())
    }
}

/// Render one workflow document from IR under a policy gate.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid context, IR, policy, or steps.
pub fn render_workflow_ir(
    ir: &WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<String, RenderError> {
    ctx.validate()?;
    ir.validate().map_err(RenderError::Contract)?;
    check_triggers(&ir.triggers)?;
    check_concurrency(&ir.concurrency)?;
    check_single_label(ir, &ctx.runs_on)?;
    let mut jobs = ir.jobs.clone();
    match policy {
        WorkflowPolicy::ConsumerV1 => support::reject_consumer_support(&jobs, support)?,
        WorkflowPolicy::VelnorRepositoryV1 => {
            support::merge_support_jobs(&mut jobs, support, ctx)?;
        }
    }
    support::check_candidate_invariants(&jobs)?;
    support::check_final_gate(&jobs)?;
    let document = document::workflow_to_yaml(ir, &jobs, ctx)?;
    let document = quote_run_values_in_yaml(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

/// Assemble the exact two-file tree from rendered workflow bytes plus the
/// actionlint crate's bytes (passed through, marker-checked).
///
/// # Errors
///
/// Returns [`RenderError`] for marker, token, or path failures.
pub fn render_tree(
    workflow_bytes: &str,
    actionlint_bytes: &str,
    version: &str,
) -> Result<RenderedTree, RenderError> {
    marker::check_first_line(workflow_bytes, version)?;
    marker::check_first_line(actionlint_bytes, version)?;
    steps::scan_for_private_subcommands(workflow_bytes)?;
    steps::scan_for_private_subcommands(actionlint_bytes)?;
    guard::validate_tree_path(ACTIONLINT_PATH)?;
    guard::validate_tree_path(WORKFLOW_PATH)?;
    Ok(RenderedTree {
        files: vec![
            RenderedFile {
                path: ACTIONLINT_PATH.to_owned(),
                bytes: actionlint_bytes.to_owned(),
            },
            RenderedFile {
                path: WORKFLOW_PATH.to_owned(),
                bytes: workflow_bytes.to_owned(),
            },
        ],
    })
}

/// Quote bare env paths in every `run:` scalar.
fn quote_run_values_in_yaml(node: Yaml) -> Yaml {
    match node {
        Yaml::Map(entries) => Yaml::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    if key == "run" {
                        if let Yaml::Str(line) = value {
                            (key, Yaml::Str(steps::quote_run_line_env_paths(&line)))
                        } else {
                            (key, value)
                        }
                    } else {
                        (key, quote_run_values_in_yaml(value))
                    }
                })
                .collect(),
        ),
        Yaml::Seq(items) => Yaml::Seq(items.into_iter().map(quote_run_values_in_yaml).collect()),
        other => other,
    }
}

/// Require a literal versioned Ubuntu label (no aliases or expressions).
fn validate_runs_on(label: &str) -> Result<(), RenderError> {
    let pinned = !label.is_empty()
        && label.starts_with("ubuntu-")
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'));
    if pinned {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "unpinned_label:{label}"
        )))
    }
}

/// Require the staged path with the exact generator version suffix.
fn validate_staged_binary(staged: &str, version: &str) -> Result<(), RenderError> {
    match staged.strip_prefix(steps::STAGED_BINARY_PREFIX) {
        Some(suffix) if suffix == version => Ok(()),
        _ => Err(RenderError::InvalidWorkflow(format!(
            "unstaged_binary:{staged}"
        ))),
    }
}

/// Require a runner-temp request directory without traversal.
fn validate_request_dir(dir: &str) -> Result<(), RenderError> {
    match dir.strip_prefix(steps::REQUEST_DIR_PREFIX) {
        Some(rest)
            if !rest.is_empty()
                && !rest.split('/').any(|seg| seg.is_empty() || seg == "..")
                && !rest.chars().any(|ch| ch.is_whitespace() || ch.is_control()) =>
        {
            Ok(())
        }
        _ => Err(RenderError::InvalidWorkflow(format!(
            "bad_request_dir:{dir}"
        ))),
    }
}

/// Require the exact trigger shape: 4 PR types, one push branch, merge group.
fn check_triggers(triggers: &Trigger) -> Result<(), RenderError> {
    let expected: Vec<String> = EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect();
    if triggers.pull_request_types != expected {
        return Err(RenderError::InvalidWorkflow("bad_pr_triggers".to_owned()));
    }
    let branch_ok = triggers.push_branches.len() == 1
        && triggers.push_branches.first().is_some_and(|branch| {
            !branch.trim().is_empty() && !branch.chars().any(char::is_whitespace)
        });
    if !branch_ok {
        return Err(RenderError::InvalidWorkflow("bad_push_branch".to_owned()));
    }
    if !triggers.merge_group {
        return Err(RenderError::InvalidWorkflow(
            "missing_merge_group".to_owned(),
        ));
    }
    Ok(())
}

/// Require the exact concurrency group plus PR-only cancel.
fn check_concurrency(concurrency: &Concurrency) -> Result<(), RenderError> {
    if concurrency.group != CONCURRENCY_GROUP
        || concurrency.cancel_in_progress != CONCURRENCY_CANCEL
    {
        return Err(RenderError::InvalidWorkflow("bad_concurrency".to_owned()));
    }
    Ok(())
}

/// Require every job to use the single context label.
fn check_single_label(ir: &WorkflowIr, label: &str) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        if job.runs_on != label {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    Ok(())
}
