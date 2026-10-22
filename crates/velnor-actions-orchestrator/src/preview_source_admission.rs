//! Two-phase, source-bound preview admission. Phase one authenticates remote
//! CI and workflow identities; phase two reads only regular files from the
//! pinned source checkout and emits the immutable admission record.

use super::{
    Admission, append_output, positive_u64, required, runner_root, source_files::SourceFiles,
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_POLICY_SCHEMA_V1, PackageReleasePlanProduct, PackageReleasePolicy,
    canonical_json_bytes,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProducer},
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_identity::PreviewIdentity,
    preview_production::{ReleaseProductionContext, expected_release_payload_pairs},
};
use velnor_actions_mise::CheckDeadline;

use super::super::{
    github::GitHubApi,
    run_identity::{CurrentRun, RunSource},
};

/// Native operation that finalizes the selected preview source admission.
pub const FINALIZE_PREVIEW_SOURCE_OP: &str = "finalize-preview-source-v1";

/// Select source A and current workflow authority B from authenticated GitHub facts.
/// # Errors
/// Rejects an ineligible CI source, workflow authority, repository, or ancestry.
pub fn select_preview_source_from_env() -> Result<(), OrchestratorError> {
    select()
}

/// Finalize A's static policy, task closure, and digest-bound source artifact.
/// # Errors
/// Rejects checkout, policy, producer-task, source-closure, or authentication drift.
pub fn finalize_preview_source_from_env() -> Result<(), OrchestratorError> {
    finalize()
}

const ADMISSION_FILE: &str = "preview-source-admission.json";
const SOURCE_SEED_POLICY_FILE: &str = "preview-source-seed-policy.json";
const MAX_ADMISSION_BYTES: usize = 16 * 1024 * 1024;
const MAX_MANIFEST_CLOSURE_BYTES: usize = 8 * 1024 * 1024;

type SourceEntryMap = BTreeMap<String, super::source_files::SourceFileEntry>;

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

struct WorkspaceSource<'a> {
    record: velnor_actions_rust::WorkspaceRecord,
    reader: SourceFiles<'a>,
    manifests: BTreeMap<String, Vec<u8>>,
}

/// Authenticate a successful main CI source and the current workflow authority.
/// This phase runs before checkout and emits only remote identities.
pub(super) fn select() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("preview_source_deadline"))?;
    deadline
        .scope(|| select_until(deadline))
        .map_err(|_| internal("preview_source_deadline"))?
}

fn select_until(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let seed = parse_seed_policy()?;
    let repository = required("GITHUB_REPOSITORY")?;
    let coordinator = required("GITHUB_SHA")?;
    if seed.repository != repository
        || seed.channel != PackageReleaseChannel::Preview
        || seed.default_branch != "main"
        || coordinator != required("GITHUB_WORKFLOW_SHA")?
        || !valid_git_sha(&coordinator)
        || required("GITHUB_REF")? != "refs/heads/main"
        || required("GITHUB_WORKFLOW_REF")?
            != format!(
                "{repository}/{}@refs/heads/main",
                seed.channel.workflow_path()
            )
    {
        return Err(internal("preview_source_authority_context"));
    }
    let event = event_json()?;
    let repository_id = positive_u64("GITHUB_REPOSITORY_ID")?;
    let current_run = positive_u64("GITHUB_RUN_ID")?;
    let current_attempt = u32::try_from(positive_u64("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("preview_source_context"))?;
    let (source_sha, ci_run_id, ci_attempt, authority) =
        source_from_invocation(&event, repository_id, &seed)?;
    if !valid_git_sha(&source_sha) || !valid_git_sha(&authority) {
        return Err(internal("preview_source_sha"));
    }

    let api = GitHubApi::new(repository);
    api.verify_repository_identity(repository_id)?;
    api.verify_preview_main_ancestry(
        repository_id,
        &seed.default_branch,
        &source_sha,
        &coordinator,
    )?;
    let ci_source = RunSource {
        repository_id,
        sha: &source_sha,
        branch: &seed.default_branch,
    };
    api.verify_ci_identity(ci_run_id, ci_attempt, ci_source, true)?;

    let workflow =
        api.admitted_workflow(seed.channel.workflow_path(), seed.channel.workflow_name())?;
    let current_source = RunSource {
        repository_id,
        sha: &coordinator,
        branch: &seed.default_branch,
    };
    let role = release_job_name_seed(seed.channel, &required("GITHUB_JOB")?)?;
    api.verify_current_role(
        &CurrentRun {
            id: current_run,
            attempt: current_attempt,
            source: current_source,
            workflow: &workflow,
            event: &required("GITHUB_EVENT_NAME")?,
        },
        &role,
    )?;
    deadline
        .remaining()
        .map_err(|_| internal("preview_source_deadline"))?;
    append_output(&format!(
        "source_sha={source_sha}\nworkflow_authority_sha={authority}\nci_run_id={ci_run_id}\nci_attempt={ci_attempt}\n"
    ))
}

fn source_from_invocation(
    event: &Value,
    repository_id: u64,
    seed: &PackageReleasePolicy,
) -> Result<(String, u64, u32, String), OrchestratorError> {
    match required("GITHUB_EVENT_NAME")?.as_str() {
        "workflow_run" => {
            let (source, run, attempt) =
                source_from_workflow_run_event(event, repository_id, &seed.repository)?;
            Ok((source, run, attempt, required("GITHUB_SHA")?))
        }
        "workflow_dispatch" => {
            let signed = super::super::recovery::verify_preview_original_for_source_selection(
                seed,
                super::super::recovery::PreviewRecoveryVerificationStage::SourceSelection,
            )?;
            let receipt = signed.receipt();
            let source_sha = receipt.source.commit_sha.clone();
            let api = GitHubApi::new(seed.repository.clone());
            let workflow = api.admitted_workflow(
                velnor_actions_contract::CI_WORKFLOW_PATH,
                velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            )?;
            let latest = api.latest_source_ci(
                &workflow,
                RunSource {
                    repository_id,
                    sha: &source_sha,
                    branch: &seed.default_branch,
                },
            )?;
            Ok((
                source_sha,
                latest
                    .id
                    .filter(|id| *id > 0)
                    .ok_or_else(|| internal("preview_source_latest_ci_identity"))?,
                latest
                    .run_attempt
                    .filter(|attempt| *attempt > 0)
                    .ok_or_else(|| internal("preview_source_latest_ci_identity"))?,
                required("GITHUB_SHA")?,
            ))
        }
        _ => Err(internal("preview_source_event_name")),
    }
}

fn source_from_workflow_run_event(
    event: &Value,
    repository_id: u64,
    repository: &str,
) -> Result<(String, u64, u32), OrchestratorError> {
    let run = &event["workflow_run"];
    if event["action"].as_str() != Some("completed")
        || event["repository"]["id"].as_u64() != Some(repository_id)
        || event["repository"]["full_name"].as_str() != Some(repository)
        || run["head_repository"]["id"].as_u64() != Some(repository_id)
        || run["head_branch"].as_str() != Some("main")
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("completed")
        || run["conclusion"].as_str() != Some("success")
        || run["path"].as_str() != Some(velnor_actions_contract::CI_WORKFLOW_PATH)
        || run["name"].as_str() != Some(velnor_actions_contract::WORKFLOW_DISPLAY_NAME)
    {
        return Err(internal("preview_source_event_identity"));
    }
    Ok((
        required_value(&run["head_sha"], "preview_source_event_identity")?,
        positive_value(&run["id"], "preview_source_event_identity")?,
        u32::try_from(positive_value(
            &run["run_attempt"],
            "preview_source_event_identity",
        )?)
        .map_err(|_| internal("preview_source_event_identity"))?,
    ))
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
    let state = Finalization {
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
    };
    authenticate_finalization(&state)?;
    Ok(state)
}

fn validate_finalize_environment(seed: &PackageReleasePolicy) -> Result<(), OrchestratorError> {
    let source = required("VELNOR_RELEASE_SOURCE_SHA")?;
    let authority = required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?;
    let coordinator = required("GITHUB_SHA")?;
    let event = required("GITHUB_EVENT_NAME")?;
    if seed.channel != PackageReleaseChannel::Preview
        || seed.default_branch != "main"
        || !valid_git_sha(&source)
        || !valid_git_sha(&authority)
        || coordinator != required("GITHUB_WORKFLOW_SHA")?
        || authority != coordinator
        || seed.repository != required("GITHUB_REPOSITORY")?
        || required("GITHUB_REF")? != "refs/heads/main"
        || required("GITHUB_WORKFLOW_REF")?
            != format!(
                "{}/{}@refs/heads/main",
                seed.repository,
                seed.channel.workflow_path()
            )
        || !matches!(event.as_str(), "workflow_run" | "workflow_dispatch")
    {
        return Err(internal("preview_source_finalize_context"));
    }
    Ok(())
}

fn authenticate_finalization(state: &Finalization) -> Result<(), OrchestratorError> {
    state.api.verify_repository_identity(state.repository_id)?;
    let coordinator = required("GITHUB_SHA")?;
    state.api.verify_preview_main_ancestry(
        state.repository_id,
        &state.seed.default_branch,
        &state.source_sha,
        &coordinator,
    )?;
    if required("GITHUB_EVENT_NAME")? == "workflow_dispatch" {
        let signed = super::super::recovery::verify_preview_original_for_source_selection(
            &state.seed,
            super::super::recovery::PreviewRecoveryVerificationStage::Finalization,
        )?;
        let receipt = signed.receipt();
        if receipt.source.commit_sha != state.source_sha
            || receipt.version.commit_count != Some(state.count)
        {
            return Err(internal("preview_source_recovery_receipt_mismatch"));
        }
    } else if state.authority != coordinator {
        return Err(internal("preview_source_authority_context"));
    }
    let workflow = state.api.admitted_workflow(
        state.seed.channel.workflow_path(),
        state.seed.channel.workflow_name(),
    )?;
    let authority_source = RunSource {
        repository_id: state.repository_id,
        sha: &coordinator,
        branch: &state.seed.default_branch,
    };
    state.api.verify_current_role(
        &CurrentRun {
            id: state.run_id,
            attempt: state.run_attempt,
            source: authority_source,
            workflow: &workflow,
            event: &required("GITHUB_EVENT_NAME")?,
        },
        "Select authenticated main source for preview",
    )?;
    state.api.verify_ci_identity(
        state.ci_run_id,
        state.ci_attempt,
        RunSource {
            repository_id: state.repository_id,
            sha: &state.source_sha,
            branch: &state.seed.default_branch,
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
        policy_sha256: super::super::github::sha256(
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
    let admission_bytes = canonical_json_bytes(admission)
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
        super::super::github::sha256(&admission_bytes),
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

fn derive_a_policy(
    api: &GitHubApi,
    workspace: &Path,
    release: &RustBinaryReleaseConfig,
    repository: &str,
    branch: &str,
) -> Result<
    (
        PackageReleasePolicy,
        velnor_actions_contract::PackageReleasePlan,
        SourceEntryMap,
    ),
    OrchestratorError,
> {
    let source_sha = required("VELNOR_RELEASE_SOURCE_SHA")?;
    let mut source = source_workspace_record(api, workspace, release, &source_sha)?;
    let resolved = velnor_actions_rust::resolve_release_products(release, &source.record)
        .map_err(|_| internal("preview_source_products"))?;
    let version = velnor_actions_rust::release_version_policy(release, &source.record)
        .map_err(|_| internal("preview_source_version_policy"))?;
    let mut products = release
        .products
        .iter()
        .map(|product| {
            let mut bins = product.bins.clone();
            bins.sort();
            let mut targets = product.targets.clone();
            targets.sort();
            PackageReleasePlanProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins,
                targets,
                format: PackageReleasePayloadFormat::TarGz,
            }
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    let mut companions = release.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    let policy = PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: repository.to_owned(),
        default_branch: branch.to_owned(),
        version,
        products,
        companions,
    };
    policy
        .validate()
        .map_err(|_| internal("preview_source_policy"))?;
    let plan = super::resolve_policy_at_source(&policy, &source_sha)?;
    if resolved.base_version.is_empty() || plan.base_version != resolved.base_version {
        return Err(internal("preview_source_policy_resolution"));
    }
    let mut entries = BTreeMap::new();
    for path in source.manifests.keys() {
        entries.insert(path.clone(), source.reader.identity(path)?);
    }
    Ok((policy, plan, entries))
}

fn source_workspace_record<'a>(
    api: &'a GitHubApi,
    workspace: &Path,
    release: &RustBinaryReleaseConfig,
    source_sha: &str,
) -> Result<WorkspaceSource<'a>, OrchestratorError> {
    let mut reader = SourceFiles::new(api, source_sha)?;
    let all_files = reader.regular_files()?;
    let regular_paths = all_files
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let root_manifest = reader.read(&release.manifest_path)?;
    let mut manifest_bytes = root_manifest.len();
    if read_local(workspace, &release.manifest_path)? != root_manifest {
        return Err(internal("preview_source_manifest_checkout_mismatch"));
    }
    let manifest_paths = velnor_actions_rust::release_workspace_manifest_paths(
        &release.manifest_path,
        &root_manifest,
        &regular_paths,
    )
    .map_err(|_| internal("preview_source_manifest_projection"))?;
    let mut manifests = BTreeMap::new();
    manifests.insert(release.manifest_path.clone(), root_manifest);
    for path in manifest_paths {
        if manifests.contains_key(&path) {
            continue;
        }
        let bytes = reader.read(&path)?;
        manifest_bytes = manifest_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_MANIFEST_CLOSURE_BYTES)
            .ok_or_else(|| internal("preview_source_manifest_closure_size"))?;
        if read_local(workspace, &path)? != bytes {
            return Err(internal("preview_source_manifest_checkout_mismatch"));
        }
        manifests.insert(path, bytes);
    }
    let workspace_record =
        velnor_actions_rust::project_release_workspace(release, &manifests, &regular_paths)
            .map_err(|_| internal("preview_source_workspace_projection"))?;
    Ok(WorkspaceSource {
        record: workspace_record,
        reader,
        manifests,
    })
}

fn payload_matrix(
    plan: &velnor_actions_contract::PackageReleasePlan,
    producer: &RustBinaryReleaseProducer,
) -> Result<String, OrchestratorError> {
    let pairs = expected_release_payload_pairs(plan)
        .map_err(|_| internal("preview_source_payload_pairs"))?;
    if pairs.is_empty() || pairs.len() > 128 {
        return Err(internal("preview_source_payload_pairs"));
    }
    let timeout = u32::from(producer.timeout_minutes)
        + u32::from(RustBinaryReleaseProducer::JOB_HEADROOM_MINUTES);
    let rows = pairs
        .iter()
        .map(|pair| {
            serde_json::json!({
                "product_id": pair.product_id,
                "target": pair.target,
                "runner": producer.runner.runs_on(),
                "timeout_minutes": timeout,
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({"include": rows});
    serde_json::to_string(&value).map_err(|_| internal("preview_source_matrix_encoding"))
}

fn parse_seed_policy() -> Result<PackageReleasePolicy, OrchestratorError> {
    let policy_text = required("VELNOR_RELEASE_POLICY")?;
    let expected_path = runner_root()?.join(SOURCE_SEED_POLICY_FILE);
    if Path::new(&required("VELNOR_REQUEST_FILE")?) != expected_path {
        return Err(internal("preview_source_policy_seed_path"));
    }
    let bytes =
        crate::retrieve_reports::staged_reads::read_staged_bytes(&expected_path, 1024 * 1024)
            .map_err(|_| internal("preview_source_policy_seed_read"))?;
    let policy = velnor_actions_contract::parse_package_release_policy(policy_text.as_bytes())
        .map_err(|_| internal("preview_source_policy_seed"))?;
    let canonical = policy
        .canonical_bytes()
        .map_err(|_| internal("preview_source_policy_seed"))?;
    if bytes != canonical || policy_text.as_bytes() != canonical.as_slice() {
        return Err(internal("preview_source_policy_seed_mismatch"));
    }
    Ok(policy)
}

fn event_json() -> Result<Value, OrchestratorError> {
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes(
        Path::new(&required("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
    )
    .map_err(|_| internal("preview_source_event_read"))?;
    velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
        .map_err(|_| internal("preview_source_event_json"))
}

fn verify_checkout(
    workspace: &Path,
    source_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let head = velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()])
        .run_in_until(workspace, deadline)
        .map_err(|_| internal("preview_source_checkout_head"))?;
    if !head.success
        || head
            .stdout_text("git")
            .map_err(|_| internal("preview_source_checkout_head"))?
            .trim()
            != source_sha
    {
        return Err(internal("preview_source_checkout_head"));
    }
    let shallow =
        velnor_actions_mise::GitRequest::rev_parse(vec!["--is-shallow-repository".into()])
            .run_in_until(workspace, deadline)
            .map_err(|_| internal("preview_source_checkout_history"))?;
    if !shallow.success
        || shallow
            .stdout_text("git")
            .map_err(|_| internal("preview_source_checkout_history"))?
            .trim()
            != "false"
    {
        return Err(internal("preview_source_checkout_history"));
    }
    Ok(())
}

fn read_local(root: &Path, path: &str) -> Result<Vec<u8>, OrchestratorError> {
    let read = crate::safe_read::read_repo_file(root, path, 1024 * 1024)?;
    match read {
        crate::safe_read::RepoRead::Text(text) => Ok(text.into_bytes()),
        crate::safe_read::RepoRead::Absent => Err(internal("preview_source_file_missing")),
    }
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn output_value(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn positive_value(value: &Value, reason: &'static str) -> Result<u64, OrchestratorError> {
    value
        .as_u64()
        .filter(|number| *number > 0)
        .ok_or_else(|| internal(reason))
}

fn required_value(value: &Value, reason: &'static str) -> Result<String, OrchestratorError> {
    value
        .as_str()
        .filter(|value| valid_git_sha(value))
        .map(str::to_owned)
        .ok_or_else(|| internal(reason))
}

fn release_job_name_seed(
    channel: PackageReleaseChannel,
    job: &str,
) -> Result<String, OrchestratorError> {
    if channel == PackageReleaseChannel::Preview && job == "preview-eligibility" {
        return Ok("Select authenticated main source for preview".to_owned());
    }
    Err(internal("preview_source_role"))
}

fn _file_mode_is_regular(mode: &str) -> bool {
    matches!(mode, "100644" | "100755")
}

#[cfg(test)]
#[path = "preview_source_admission_tests.rs"]
mod tests;
