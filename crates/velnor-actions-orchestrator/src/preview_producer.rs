//! Credential-free execution of the declared native preview producer.

use crate::OrchestratorError;
use crate::check_evidence::reject_link_components;
use crate::check_runtime::preparation::prepare_check;
use crate::exclusive_write::create_dir_no_symlink;
use crate::internal::internal;
use serde::Serialize;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::time::Instant;
use velnor_actions_contract::config::RustBinaryPreviewConfig;
use velnor_actions_contract::preview_identity::PreviewIdentity;
use velnor_actions_mise::{CheckDeadline, GitRequest, IsolatedCommand, ProcessOutput};

#[path = "preview_producer_inputs.rs"]
mod inputs;
use inputs::require_tracked_clean_input;
#[cfg(test)]
use inputs::valid_stage_zero_regular_file;

const MAX_TASK_OUTPUT: usize = 8 * 1024 * 1024;

#[path = "preview_producer_entry.rs"]
mod entry;
pub use entry::{EXECUTE_PREVIEW_PRODUCER_OP, execute_preview_producer};

/// Evidence retained after one isolated producer task succeeds.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewProducerProof {
    pub source_sha: String,
    pub source_ref: String,
    pub version: String,
    pub commit_count: u64,
    pub task: String,
    pub task_exit_code: i32,
    pub duration_ms: u64,
    pub output_directory: String,
    pub output_manifest: String,
    pub qualification_digest: String,
    pub system_tools: Vec<velnor_actions_mise::checks::SystemToolProof>,
    pub qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
}

/// Execute exactly the configured producer task in a fresh isolated tool home.
///
/// The producer receives only the fixed release handoff variables and its
/// validated application version variable. It cannot supply task arguments or
/// arbitrary environment additions. The output root is created exclusively
/// beneath the repository after refusing preexisting content.
/// # Errors
/// Rejects a changed checkout, untracked/modified/linked inputs, unsafe output
/// paths, failed task execution, or incomplete tool qualification.
fn execute_preview_task(
    root: &Path,
    temp: &Path,
    identity: &PreviewIdentity,
    source_ref: &str,
    preview: &RustBinaryPreviewConfig,
    qualified_tools: &[velnor_actions_contract::config::QualifiedTool],
    deadline: CheckDeadline,
) -> Result<PreviewProducerProof, OrchestratorError> {
    checkpoint(deadline)?;
    preview.validate(".velnor/config.toml")?;
    if !preview.enabled || !valid_source_ref(source_ref) || !canonical_identity(identity) {
        return Err(internal("preview_producer_identity"));
    }
    let producer = &preview.producer;
    let root = root.canonicalize().map_err(|_| internal("preview_root"))?;
    validate_preview_source(&root, identity, deadline)?;
    let discovered =
        velnor_actions_mise::discover_preview_producer(&root, producer, qualified_tools, deadline)
            .map_err(|error| internal(&error.to_string()))?;
    if discovered.check.runner.platform.os() != std::env::consts::OS
        || discovered.check.runner.platform.arch() != std::env::consts::ARCH
    {
        return Err(internal("preview_producer_host_platform"));
    }

    let mut inputs: BTreeSet<String> = producer.inputs.iter().cloned().collect();
    inputs.extend(discovered.config_inputs.iter().cloned());
    validate_inputs(&root, &inputs, false, deadline)?;

    let output = root.join(&producer.output_directory);
    reject_link_components(&root, &producer.output_directory)?;
    if fs::symlink_metadata(&output).is_ok() {
        return Err(internal("preview_output_preexisting"));
    }
    let parent = output
        .parent()
        .ok_or_else(|| internal("preview_output_path"))?;
    create_dir_no_symlink(&root, parent)?;
    fs::create_dir(&output).map_err(|_| internal("preview_output_create"))?;
    reject_link_components(&root, &producer.output_directory)?;

    let started = Instant::now();
    let owned = prepare_check(&root, temp, &discovered, deadline)?;
    let pairs = producer_handoff(&root, &output, preview, identity, source_ref)?;
    let command = owned
        .command(deadline)
        .map_err(|error| internal(&error.to_string()))?
        .with_env(&pairs)
        .map_err(|error| internal(&error.to_string()))?;
    let task_output = run_preview_task(&command, deadline)?;
    validate_inputs(&root, &inputs, true, deadline)?;
    validate_preview_source(&root, identity, deadline)?;
    checkpoint(deadline)?;
    let task_exit_code = task_output.code.unwrap_or(-1);
    Ok(PreviewProducerProof {
        source_sha: identity.source_sha.clone(),
        source_ref: source_ref.to_owned(),
        version: identity.version.clone(),
        commit_count: identity.commit_count,
        task: producer.task.clone(),
        task_exit_code,
        duration_ms: u64::try_from(started.elapsed().as_millis())
            .unwrap_or(u64::MAX)
            .max(1),
        output_directory: producer.output_directory.clone(),
        output_manifest: producer.manifest.clone(),
        qualification_digest: discovered.qualification_digest,
        system_tools: owned.system_tools.clone(),
        qualified_tools: owned.qualified_tools.clone(),
    })
}

fn run_preview_task(
    command: &IsolatedCommand,
    deadline: CheckDeadline,
) -> Result<ProcessOutput, OrchestratorError> {
    let output = command
        .run_until(MAX_TASK_OUTPUT, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    output
        .require_success("preview-producer")
        .map_err(|error| internal(&error.to_string()))?;
    Ok(output)
}

fn producer_handoff(
    root: &Path,
    output: &Path,
    preview: &RustBinaryPreviewConfig,
    identity: &PreviewIdentity,
    source_ref: &str,
) -> Result<Vec<(OsString, OsString)>, OrchestratorError> {
    let output_directory = output
        .to_str()
        .ok_or_else(|| internal("preview_output_utf8"))?;
    let output_manifest = root.join(&preview.producer.manifest);
    let output_manifest = output_manifest
        .to_str()
        .ok_or_else(|| internal("preview_manifest_utf8"))?;
    let mut pairs: Vec<(OsString, OsString)> = [
        ("VELNOR_RELEASE_SOURCE_COMMIT", identity.source_sha.as_str()),
        ("VELNOR_RELEASE_SOURCE_REF", source_ref),
        ("VELNOR_RELEASE_VERSION", identity.version.as_str()),
        ("VELNOR_RELEASE_OUTPUT_DIRECTORY", output_directory),
        ("VELNOR_RELEASE_OUTPUT_MANIFEST", output_manifest),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    pairs.extend([
        (
            "VELNOR_RELEASE_COMMIT_COUNT".into(),
            identity.commit_count.to_string().into(),
        ),
        (
            "CARGO_BUILD_JOBS".into(),
            preview.producer.build_jobs.to_string().into(),
        ),
        (
            preview.version_env.clone().into(),
            identity.version.clone().into(),
        ),
    ]);
    Ok(pairs)
}

fn validate_inputs(
    root: &Path,
    inputs: &BTreeSet<String>,
    after_task: bool,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for input in inputs {
        checkpoint(deadline)?;
        reject_link_components(root, input)?;
        let missing = if after_task {
            "preview_input_missing_after_task"
        } else {
            "preview_input_missing"
        };
        let regular = if after_task {
            "preview_input_not_regular_after_task"
        } else {
            "preview_input_not_regular"
        };
        let metadata = fs::symlink_metadata(root.join(input)).map_err(|_| internal(missing))?;
        if !metadata.file_type().is_file() {
            return Err(internal(regular));
        }
        require_tracked_clean_input(root, input, deadline)?;
    }
    Ok(())
}

fn validate_preview_source(
    root: &Path,
    identity: &PreviewIdentity,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if current_head(root, deadline)? != identity.source_sha {
        return Err(internal("preview_source_head_mismatch"));
    }
    let clean = GitRequest::diff(
        [
            "--no-ext-diff",
            "--no-textconv",
            "--quiet",
            "--exit-code",
            "HEAD",
            "--",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    )
    .run_in_until(root, deadline)
    .map_err(|error| internal(&error.to_string()))?;
    if !clean.success {
        return Err(internal("preview_source_tree_changed"));
    }
    let shallow = GitRequest::rev_parse(vec![OsString::from("--is-shallow-repository")])
        .run_in_until(root, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    if !shallow.success
        || shallow
            .stdout_text("git")
            .map_err(|error| internal(&error.to_string()))?
            .trim()
            != "false"
    {
        return Err(internal("preview_requires_full_git_history"));
    }
    let count = GitRequest::rev_list_count(&identity.source_sha)
        .map_err(|error| internal(&error.to_string()))?
        .run_in_until(root, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    if !count.success
        || count
            .stdout_text("git")
            .map_err(|error| internal(&error.to_string()))?
            .trim()
            .parse::<u64>()
            .ok()
            != Some(identity.commit_count)
    {
        return Err(internal("preview_source_commit_count_mismatch"));
    }
    Ok(())
}

pub(super) fn source_commit_count(
    root: &Path,
    source_sha: &str,
    deadline: CheckDeadline,
) -> Result<u64, OrchestratorError> {
    if source_sha.len() != 40
        || !source_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(internal("preview_source_sha_invalid"));
    }
    let shallow = GitRequest::rev_parse(vec![OsString::from("--is-shallow-repository")])
        .run_in_until(root, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    if !shallow.success
        || shallow
            .stdout_text("git")
            .map_err(|error| internal(&error.to_string()))?
            .trim()
            != "false"
    {
        return Err(internal("preview_requires_full_git_history"));
    }
    let count = GitRequest::rev_list_count(source_sha)
        .map_err(|error| internal(&error.to_string()))?
        .run_in_until(root, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    if !count.success {
        return Err(internal("preview_source_commit_count_unavailable"));
    }
    count
        .stdout_text("git")
        .map_err(|error| internal(&error.to_string()))?
        .trim()
        .parse::<u64>()
        .map_err(|_| internal("preview_source_commit_count_invalid"))
}

fn current_head(root: &Path, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    let result = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .run_in_until(root, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    if !result.success {
        return Err(internal("preview_head_unavailable"));
    }
    Ok(result
        .stdout_text("git")
        .map_err(|error| internal(&error.to_string()))?
        .trim()
        .to_owned())
}

fn canonical_identity(identity: &PreviewIdentity) -> bool {
    let suffix = format!(
        "-preview.{}+{}",
        identity.commit_count,
        identity.source_sha.get(..7).unwrap_or_default()
    );
    identity.version.strip_suffix(&suffix).is_some_and(|base| {
        PreviewIdentity::derive(base, &identity.source_sha, identity.commit_count)
            .is_ok_and(|expected| expected == *identity)
    })
}

fn valid_source_ref(source_ref: &str) -> bool {
    source_ref
        .strip_prefix("refs/heads/")
        .is_some_and(velnor_actions_contract::is_valid_branch_name)
}

fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}

#[cfg(test)]
#[path = "preview_producer_tests.rs"]
mod tests;
