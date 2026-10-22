use super::super::super::{
    github::GitHubApi,
    run_identity::{CurrentRun, RunSource},
};
use super::super::source_files::SourceFiles;
use super::super::{Admission, append_output, positive_u64, required, runner_root};
use super::MAX_MANIFEST_CLOSURE_BYTES;
use super::policy::{SourceEntryMap, derive_a_policy, payload_matrix};
use super::support::{output_value, parse_seed_policy, read_local, valid_git_sha, verify_checkout};
use crate::{OrchestratorError, internal::internal};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};
use velnor_actions_contract::{
    PackageReleasePolicy, canonical_json_bytes,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProducer},
    package_release::PackageReleaseChannel,
    preview_identity::PreviewIdentity,
    preview_production::ReleaseProductionContext,
};
use velnor_actions_mise::CheckDeadline;

/// Operation identifier for the source-bound preview finalization step.
pub const FINALIZE_PREVIEW_SOURCE_OP: &str = "finalize-preview-source-v1";
const ADMISSION_FILE: &str = "preview-source-admission.json";
const MAX_ADMISSION_BYTES: usize = 16 * 1024 * 1024;

struct Finalization {
    seed: PackageReleasePolicy,
    source_sha: String,
    authority: String,
    ci_run_id: u64,
    ci_attempt: u32,
    count: u64,
    repository_id: u64,
    run_id: u64,
    run_attempt: u32,
    workspace: PathBuf,
    api: GitHubApi,
    release: RustBinaryReleaseConfig,
    qualified_tools: Vec<velnor_actions_contract::config::QualifiedTool>,
}

#[derive(Clone, Copy)]
struct FinalizationRunIdentity<'a> {
    source_sha: &'a str,
    authority: &'a str,
    ci_run_id: u64,
    ci_attempt: u32,
    repository_id: u64,
    run_id: u64,
    run_attempt: u32,
}

/// Finalize A's static policy, task closure, and digest-bound source artifact.
/// # Errors
/// Rejects checkout, policy, producer-task, source-closure, or authentication drift.
pub fn finalize_preview_source_from_env() -> Result<(), OrchestratorError> {
    finalize()
}

/// Verify A checkout custody, derive A policy/task closure, and write the one
/// canonical artifact consumed by each source-bound production job.
pub(super) fn finalize() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("preview_source_deadline"))?;
    deadline
        .scope(|| finalize_until(deadline))
        .map_err(|_| internal("preview_source_deadline"))?
}

fn finalize_until(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let state = prepare_finalization(deadline)?;
    let (policy, plan, mut entries) = derive_a_policy(
        &state.api,
        &state.workspace,
        &state.release,
        &state.seed.repository,
        &state.seed.default_branch,
    )?;
    let discovered = discover_source_producer(&state, deadline)?;
    entries.extend(source_closure_entries(
        &state,
        &policy,
        &discovered.config_inputs,
    )?);
    let admission = make_admission(&state, &policy, &plan, discovered, entries)?;
    emit_finalization(&state, &policy, &plan, &admission)
}

fn prepare_finalization(deadline: CheckDeadline) -> Result<Finalization, OrchestratorError> {
    let seed = parse_seed_policy()?;
    validate_finalize_environment(&seed)?;
    let source_sha = required("VELNOR_RELEASE_SOURCE_SHA")?;
    let authority = required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?;
    let workspace = PathBuf::from(required("GITHUB_WORKSPACE")?)
        .canonicalize()
        .map_err(|_| internal("preview_source_workspace"))?;
    verify_checkout(&workspace, &source_sha, deadline)?;
    let count = crate::preview_producer::source_commit_count(&workspace, &source_sha, deadline)?;
    let config = crate::config::load_config(&workspace)?;
    let release = config
        .stacks
        .rust
        .as_ref()
        .map(|rust| rust.binary_release.clone())
        .ok_or_else(|| internal("preview_source_config_missing"))?;
    release.validate(".velnor/config.toml")?;
    if !release.channel_enabled(PackageReleaseChannel::Preview) {
        return Err(internal("preview_source_channel_disabled"));
    }
    let ci_run_id = positive_u64("VELNOR_RELEASE_CI_RUN_ID")?;
    let ci_attempt = u32::try_from(positive_u64("VELNOR_RELEASE_CI_RUN_ATTEMPT")?)
        .map_err(|_| internal("preview_source_context"))?;
    let repository_id = positive_u64("GITHUB_REPOSITORY_ID")?;
    let run_id = positive_u64("GITHUB_RUN_ID")?;
    let run_attempt = u32::try_from(positive_u64("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("preview_source_context"))?;
    let api = GitHubApi::new(seed.repository.clone());
    authenticate_finalization(
        &api,
        &seed,
        FinalizationRunIdentity {
            source_sha: &source_sha,
            authority: &authority,
            ci_run_id,
            ci_attempt,
            repository_id,
            run_id,
            run_attempt,
        },
    )?;
    Ok(Finalization {
        seed,
        source_sha,
        authority,
        ci_run_id,
        ci_attempt,
        count,
        repository_id,
        run_id,
        run_attempt,
        workspace,
        api,
        release,
        qualified_tools: config.qualified_tools,
    })
}

fn validate_finalize_environment(seed: &PackageReleasePolicy) -> Result<(), OrchestratorError> {
    let source = required("VELNOR_RELEASE_SOURCE_SHA")?;
    let authority = required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?;
    if seed.channel != PackageReleaseChannel::Preview
        || seed.default_branch != "main"
        || !valid_git_sha(&source)
        || authority != required("GITHUB_SHA")?
        || authority != required("GITHUB_WORKFLOW_SHA")?
        || seed.repository != required("GITHUB_REPOSITORY")?
        || required("GITHUB_REF")? != "refs/heads/main"
        || required("GITHUB_WORKFLOW_REF")?
            != format!(
                "{}/{}@refs/heads/main",
                seed.repository,
                seed.channel.workflow_path()
            )
        || !matches!(
            required("GITHUB_EVENT_NAME")?.as_str(),
            "workflow_run" | "workflow_dispatch"
        )
    {
        return Err(internal("preview_source_finalize_context"));
    }
    Ok(())
}

fn authenticate_finalization(
    api: &GitHubApi,
    seed: &PackageReleasePolicy,
    identity: FinalizationRunIdentity<'_>,
) -> Result<(), OrchestratorError> {
    let FinalizationRunIdentity {
        source_sha,
        authority,
        ci_run_id,
        ci_attempt,
        repository_id,
        run_id,
        run_attempt,
    } = identity;
    api.verify_repository_identity(repository_id)?;
    api.verify_preview_main_ancestry(repository_id, &seed.default_branch, source_sha, authority)?;
    let workflow =
        api.admitted_workflow(seed.channel.workflow_path(), seed.channel.workflow_name())?;
    let authority_source = RunSource {
        repository_id,
        sha: authority,
        branch: &seed.default_branch,
    };
    api.verify_current_role(
        &CurrentRun {
            id: run_id,
            attempt: run_attempt,
            source: authority_source,
            workflow: &workflow,
            event: &required("GITHUB_EVENT_NAME")?,
        },
        "Select authenticated main source for preview",
    )?;
    api.verify_ci_identity(
        ci_run_id,
        ci_attempt,
        RunSource {
            repository_id,
            sha: source_sha,
            branch: &seed.default_branch,
        },
        true,
    )
}

fn discover_source_producer(
    state: &Finalization,
    deadline: CheckDeadline,
) -> Result<velnor_actions_mise::DiscoveredCheck, OrchestratorError> {
    velnor_actions_mise::discover_preview_producer(
        &state.workspace,
        &state.release.producer,
        &state.qualified_tools,
        deadline,
    )
    .map_err(|_| internal("preview_source_task_discovery"))
}

fn source_closure_entries(
    state: &Finalization,
    policy: &PackageReleasePolicy,
    task_inputs: &[String],
) -> Result<SourceEntryMap, OrchestratorError> {
    let mut paths = BTreeSet::from([".velnor/config.toml".to_owned()]);
    paths.extend(policy.manifest_paths().into_iter().map(str::to_owned));
    paths.extend(task_inputs.iter().cloned());
    let mut reader = SourceFiles::new(&state.api, &state.source_sha)?;
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    for path in paths {
        let local = read_local(&state.workspace, &path)?;
        total = total
            .checked_add(local.len())
            .filter(|size| *size <= MAX_MANIFEST_CLOSURE_BYTES)
            .ok_or_else(|| internal("preview_source_closure_size"))?;
        if local != reader.read(&path)? {
            return Err(internal("preview_source_file_checkout_mismatch"));
        }
        entries.insert(path.clone(), reader.identity(&path)?);
    }
    Ok(entries)
}

fn make_admission(
    state: &Finalization,
    policy: &PackageReleasePolicy,
    plan: &velnor_actions_contract::PackageReleasePlan,
    discovered: velnor_actions_mise::DiscoveredCheck,
    entries: SourceEntryMap,
) -> Result<Admission, OrchestratorError> {
    let identity = PreviewIdentity::derive(&plan.base_version, &state.source_sha, state.count)
        .map_err(|_| internal("preview_source_identity"))?;
    let context = ReleaseProductionContext {
        repository_id: state.repository_id,
        head_repository_id: state.repository_id,
        source_sha: state.source_sha.clone(),
        source_ref: "refs/heads/main".to_owned(),
        workflow_authority_sha: state.authority.clone(),
        run_id: state.run_id,
        run_attempt: state.run_attempt,
        ci_run_id: state.ci_run_id,
        ci_run_attempt: state.ci_attempt,
        commit_count: state.count,
        version: identity.version,
    };
    let mut source_files = entries.into_values().collect::<Vec<_>>();
    source_files.sort_by(|left, right| left.path.cmp(&right.path));
    let producer = state.release.producer.clone();
    Ok(Admission {
        schema: "velnor.preview-source-admission.v1".to_owned(),
        policy_sha256: super::super::super::github::sha256(
            &policy
                .canonical_bytes()
                .map_err(|_| internal("release_policy_encoding"))?,
        ),
        plan_sha256: velnor_actions_contract::preview_production::release_plan_sha256(plan)
            .map_err(|_| internal("release_plan_encoding"))?,
        context,
        source_files,
        producer,
        task_config: discovered.task_config,
        task_inputs: discovered.config_inputs,
        tool_specs: discovered.tool_specs,
        qualified_tools: discovered.qualified_tools,
        qualification_digest: discovered.qualification_digest,
    })
}

fn emit_finalization(
    state: &Finalization,
    policy: &PackageReleasePolicy,
    plan: &velnor_actions_contract::PackageReleasePlan,
    admission: &Admission,
) -> Result<(), OrchestratorError> {
    let admission_bytes = canonical_json_bytes(&admission)
        .map_err(|_| internal("preview_source_admission_encoding"))?;
    if admission_bytes.len() > MAX_ADMISSION_BYTES {
        return Err(internal("preview_source_admission_size"));
    }
    let policy_bytes = policy
        .canonical_bytes()
        .map_err(|_| internal("release_policy_encoding"))?;
    let plan_bytes = plan
        .canonical_bytes()
        .map_err(|_| internal("release_plan_encoding"))?;
    write_finalization_files(&admission_bytes, &policy_bytes, &plan_bytes)?;
    let matrix = payload_matrix(plan, &state.release.producer)?;
    let timeout = u32::from(state.release.producer.timeout_minutes)
        + u32::from(RustBinaryReleaseProducer::JOB_HEADROOM_MINUTES);
    append_output(&format!(
        "commit_count={}\nrelease_policy={}\npayload_matrix={}\nproducer_runner={}\nproducer_timeout_minutes={timeout}\nsource_admission_sha256={}\n",
        state.count,
        output_value(
            std::str::from_utf8(&policy_bytes).map_err(|_| internal("release_policy_encoding"))?
        ),
        output_value(&matrix),
        output_value(state.release.producer.runner.runs_on()),
        super::super::super::github::sha256(&admission_bytes),
    ))
}

fn write_finalization_files(
    admission: &[u8],
    policy: &[u8],
    plan: &[u8],
) -> Result<(), OrchestratorError> {
    let temp = runner_root()?;
    for (name, bytes, label) in [
        (ADMISSION_FILE, admission, "preview_source_admission"),
        ("package-release-policy.json", policy, "release_policy"),
        ("package-release-plan.json", plan, "release_plan"),
    ] {
        crate::exclusive_write::write_exclusive(&temp.join(name), bytes, label)?;
    }
    Ok(())
}
