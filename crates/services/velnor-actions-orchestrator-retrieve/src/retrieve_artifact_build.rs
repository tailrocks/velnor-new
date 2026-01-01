//! Retrieve exact provider/task build artifacts and preserve API identity evidence.

#[path = "artifact_build_api.rs"]
mod api;
#[path = "artifact_build_download.rs"]
mod download;

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use self::api::{ApiArtifact, ApiJob};
use self::download::{create_download_dir, extract_artifact_archive, inspect_download};
use velnor_actions_contract::{canonical_json_bytes, parse_strict_json};
use velnor_actions_contract_config::ArtifactBuildTask;
use velnor_actions_contract_workflow::{
    ARTIFACT_BUILD_OBSERVATIONS_FILENAME, ArtifactBuildExpectation, ArtifactBuildIdentity,
    ArtifactBuildObservation, ArtifactBuildProvider, ArtifactBuildRunContext, JobConclusion, Plan,
    artifact_plan_providers, expected_artifact_builds,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_orchestrator_core::exclusive_write::write_exclusive;
use velnor_actions_orchestrator_core::staged_reads::read_staged_text;

use crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES;

/// Retrieve artifact-build observations for the current workflow attempt.
///
/// Fetch attempt-scoped jobs and plan-derived artifact names; verify run,
/// repository, and source SHA before download. Incomplete listings abort
/// without a receipt; missing task/artifact remains explicit for Required.
/// # Errors
/// Returns an error for an invalid plan, API inventory, artifact name, or bounded download.
pub fn retrieve_artifact_builds_to(
    run_id: u64,
    attempt: u32,
    repository: &str,
    repository_id: &str,
    run_dir: &Path,
) -> Result<usize, &'static str> {
    let plan = read_typed_plan(run_dir)?;
    if plan.artifact_tasks.is_empty() {
        return Ok(0);
    }
    if plan.run_key != format!("r{run_id}-a{attempt}") {
        return Err("artifact_plan_run_mismatch");
    }
    let context = ArtifactBuildRunContext {
        repository_id: repository_id.to_owned(),
        repository: repository.to_owned(),
        run_id: run_id.to_string(),
        run_attempt: attempt,
    };
    let providers = providers_for_plan(&plan)?;
    let expected = expected_artifact_builds(&plan, &context, &providers)
        .map_err(|_| "artifact_expected_inventory_invalid")?;
    let repo = velnor_actions_orchestrator_core::origin::validate_repository_slug(repository)
        .ok_or("artifact_bad_repository")?;
    let catalog = ToolCatalog::pinned();
    let jobs = api::list_jobs(&catalog, run_dir, &repo, run_id, attempt)?;
    let artifacts = api::list_artifacts(&catalog, run_dir, &repo, run_id)?;
    validate_api_inventory(&jobs, &artifacts, &expected, run_id, attempt)?;
    let mut observations = Vec::with_capacity(expected.len());
    let mut downloaded = 0usize;
    for expectation in &expected {
        let task = task_for_identity(&plan, &expectation.identity)?;
        let job = unique_job(&jobs, expectation);
        let artifact_name = expected_build_artifact_name(&expectation.identity)?;
        let artifact = unique_artifact(&artifacts, &artifact_name);
        let mut observation = observation_for(expectation, job, artifact);
        if download_is_bound_to_expected(&observation, &context, expectation, task) {
            let destination = create_download_dir(run_dir, &artifact_name)?;
            let archive_size = observation.api_artifact_size_bytes.unwrap_or_default();
            let artifact_result = download_and_extract_artifact(&ArtifactDownloadRequest {
                catalog: &catalog,
                run_dir,
                repo: &repo,
                artifact_id: artifact.map(|item| item.id).unwrap_or_default(),
                expected_archive_bytes: archive_size,
                max_archive_bytes: maximum_artifact_size(task).unwrap_or_default(),
                expectation,
                task,
                destination: &destination,
            })
            .and_then(|()| inspect_download(expectation, task, &destination));
            match artifact_result {
                Ok((result, outputs)) => {
                    observation.result = Some(result);
                    observation.downloaded_outputs = outputs;
                    downloaded += 1;
                }
                Err(_) => drop(fs::remove_dir_all(&destination)),
            }
        }
        observations.push(observation);
    }
    let receipt = run_dir.join(ARTIFACT_BUILD_OBSERVATIONS_FILENAME);
    let bytes = canonical_json_bytes(&observations).map_err(|_| "artifact_receipt_encode")?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RETRIEVE_PLAN_BYTES {
        return Err("artifact_receipt_oversize");
    }
    write_exclusive(&receipt, &bytes, "artifact_build_receipt")
        .map_err(|_| "artifact_receipt_write")?;
    Ok(downloaded)
}

/// Derive the safe artifact directory name from the typed expected identity.
///
/// Build artifacts have their own `velnor-build-*` identity family. The
/// general matrix/report artifact validator intentionally accepts other
/// prefixes, so the typed constructor is the authority for this name.
fn expected_build_artifact_name(identity: &ArtifactBuildIdentity) -> Result<String, &'static str> {
    velnor_actions_contract_workflow::artifact_name(identity).map_err(|_| "artifact_name_invalid")
}

fn validate_api_inventory(
    jobs: &[ApiJob],
    artifacts: &[ApiArtifact],
    expected: &[ArtifactBuildExpectation],
    run_id: u64,
    attempt: u32,
) -> Result<(), &'static str> {
    let expected_jobs: std::collections::BTreeSet<String> = expected
        .iter()
        .map(ArtifactBuildExpectation::expected_workflow_job_name)
        .collect();
    let expected_artifacts: std::collections::BTreeSet<String> = expected
        .iter()
        .map(|item| {
            velnor_actions_contract_workflow::artifact_name(&item.identity)
                .map_err(|_| "artifact_name_invalid")
        })
        .collect::<Result<_, _>>()?;
    let mut seen_jobs = std::collections::BTreeSet::new();
    for job in jobs
        .iter()
        .filter(|job| job.name.starts_with("Build artifact ") || expected_jobs.contains(&job.name))
    {
        if !expected_jobs.contains(&job.name) || !seen_jobs.insert(job.name.as_str()) {
            return Err("artifact_api_unexpected_or_duplicate_job");
        }
    }
    let prefix = format!("velnor-build-{run_id}-{attempt}-");
    let mut seen_artifacts = std::collections::BTreeSet::new();
    for artifact in artifacts
        .iter()
        .filter(|item| item.name.starts_with(&prefix))
    {
        if !expected_artifacts.contains(&artifact.name) || !seen_artifacts.insert(&artifact.name) {
            return Err("artifact_api_unexpected_or_duplicate_artifact");
        }
    }
    Ok(())
}

fn read_typed_plan(run_dir: &Path) -> Result<Plan, &'static str> {
    let text = read_staged_text(&run_dir.join("plan.json"), MAX_RETRIEVE_PLAN_BYTES)
        .map_err(|_| "artifact_plan_unavailable")?;
    let value = parse_strict_json(&text).map_err(|_| "artifact_plan_malformed")?;
    let plan: Plan = serde_json::from_value(value).map_err(|_| "artifact_plan_malformed")?;
    plan.validate().map_err(|_| "artifact_plan_invalid")?;
    Ok(plan)
}

fn providers_for_plan(plan: &Plan) -> Result<Vec<ArtifactBuildProvider>, &'static str> {
    let providers = artifact_plan_providers(plan);
    if providers.is_empty() {
        return Err("artifact_plan_empty");
    }
    Ok(providers)
}

fn task_for_identity<'a>(
    plan: &'a Plan,
    identity: &ArtifactBuildIdentity,
) -> Result<&'a velnor_actions_contract_config::ArtifactBuildTask, &'static str> {
    plan.artifact_tasks
        .iter()
        .find(|item| item.task.id == identity.task_id)
        .map(|item| &item.task)
        .ok_or("artifact_task_missing_from_plan")
}

fn unique_job<'a>(
    jobs: &'a [ApiJob],
    expectation: &ArtifactBuildExpectation,
) -> Option<&'a ApiJob> {
    let mut matches = jobs
        .iter()
        .filter(|job| job.name == expectation.expected_workflow_job_name());
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn unique_artifact<'a>(artifacts: &'a [ApiArtifact], name: &str) -> Option<&'a ApiArtifact> {
    let mut matches = artifacts.iter().filter(|artifact| artifact.name == name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn observation_for(
    expectation: &ArtifactBuildExpectation,
    job: Option<&ApiJob>,
    artifact: Option<&ApiArtifact>,
) -> ArtifactBuildObservation {
    let workflow_run = artifact.and_then(|item| item.workflow_run.as_ref());
    ArtifactBuildObservation {
        identity: expectation.identity.clone(),
        api_run_id: job.map_or_else(String::new, |item| item.run_id.to_string()),
        api_head_sha: job.map_or_else(String::new, |item| item.head_sha.clone()),
        api_job_id: job.map(|item| item.id),
        api_job_name: job.map_or_else(String::new, |item| item.name.clone()),
        api_job_status: job.map_or_else(|| "missing".to_owned(), |item| item.status.clone()),
        api_runner_id: job.and_then(|item| item.runner_id),
        api_runner_name: job.and_then(|item| item.runner_name.clone()),
        api_runner_group_id: job.and_then(|item| item.runner_group_id),
        api_runner_group_name: job.and_then(|item| item.runner_group_name.clone()),
        api_runner_labels: job.map_or_else(Vec::new, |item| item.labels.clone()),
        api_artifact_id: artifact.map(|item| item.id),
        api_artifact_name: artifact.map(|item| item.name.clone()),
        api_artifact_size_bytes: artifact.map(|item| item.size_in_bytes),
        api_artifact_expired: artifact.map(|item| item.expired),
        api_artifact_run_id: workflow_run.map(|item| item.id.to_string()),
        api_artifact_repository_id: workflow_run.map(|item| item.repository_id.to_string()),
        api_artifact_head_sha: workflow_run.map(|item| item.head_sha.clone()),
        conclusion: job
            .and_then(|item| item.conclusion.as_deref())
            .and_then(|value| JobConclusion::parse(value).ok())
            .unwrap_or(JobConclusion::Missing),
        result: None,
        downloaded_outputs: Vec::new(),
    }
}

fn download_is_bound_to_expected(
    observation: &ArtifactBuildObservation,
    context: &ArtifactBuildRunContext,
    expectation: &ArtifactBuildExpectation,
    task: &velnor_actions_contract_config::ArtifactBuildTask,
) -> bool {
    let artifact_size = observation.api_artifact_size_bytes;
    observation.api_job_id.is_some_and(|id| id > 0)
        && observation.api_artifact_id.is_some_and(|id| id > 0)
        && observation.api_job_status == "completed"
        && observation.conclusion == JobConclusion::Success
        && observation.api_run_id == context.run_id
        && observation.api_head_sha == expectation.identity.source_sha
        && observation.api_artifact_run_id.as_deref() == Some(context.run_id.as_str())
        && observation.api_artifact_repository_id.as_deref() == Some(context.repository_id.as_str())
        && observation.api_artifact_head_sha.as_deref()
            == Some(expectation.identity.source_sha.as_str())
        && observation.api_artifact_expired == Some(false)
        && artifact_size.is_some_and(|size| {
            maximum_artifact_size(task).is_some_and(|maximum| size > 0 && size <= maximum)
        })
}

fn maximum_artifact_size(task: &velnor_actions_contract_config::ArtifactBuildTask) -> Option<u64> {
    let raw_output_bound = task
        .outputs
        .iter()
        .try_fold(0_u64, |sum, output| sum.checked_add(output.max_bytes))?;
    let zip_metadata_bound = task.outputs.iter().try_fold(0_u64, |sum, output| {
        let name_bytes = u64::try_from(output.id.len()).ok()?;
        sum.checked_add(1024)?.checked_add(name_bytes)
    })?;
    raw_output_bound
        .checked_add(download::MAX_RESULT_BYTES)?
        .checked_add(zip_metadata_bound)
}

fn archive_download_args(repo: &str, artifact_id: u64) -> Vec<OsString> {
    [
        "api".to_owned(),
        format!("repos/{repo}/actions/artifacts/{artifact_id}/zip"),
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

#[derive(Clone, Copy)]
struct ArtifactDownloadRequest<'a> {
    catalog: &'a ToolCatalog,
    run_dir: &'a Path,
    repo: &'a str,
    artifact_id: u64,
    expected_archive_bytes: u64,
    max_archive_bytes: u64,
    expectation: &'a ArtifactBuildExpectation,
    task: &'a ArtifactBuildTask,
    destination: &'a Path,
}

fn download_and_extract_artifact(
    request: &ArtifactDownloadRequest<'_>,
) -> Result<(), &'static str> {
    let ArtifactDownloadRequest {
        catalog,
        run_dir,
        repo,
        artifact_id,
        expected_archive_bytes,
        max_archive_bytes,
        expectation,
        task,
        destination,
    } = *request;
    if artifact_id == 0
        || expected_archive_bytes == 0
        || expected_archive_bytes > max_archive_bytes
        || max_archive_bytes == 0
    {
        return Err("artifact_archive_size_invalid");
    }
    let archive_path = destination.join(".artifact-download.zip");
    let mut archive = TemporaryArchive::create(archive_path)?;
    let command = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        archive_download_args(repo, artifact_id),
    )
    .map_err(|_| "artifact_archive_command_invalid")?
    .command(catalog)
    .map_err(|_| "artifact_archive_command_invalid")?
    .with_cwd(run_dir.to_path_buf());
    let bytes_written = command
        .run_stdout_to(max_archive_bytes, Duration::from_secs(600), |chunk| {
            archive
                .file_mut()?
                .write_all(chunk)
                .map_err(|_| "archive_disk_write_failed".to_owned())
        })
        .map_err(|_| "artifact_archive_download_failed")?;
    if bytes_written != expected_archive_bytes {
        return Err("artifact_archive_size_mismatch");
    }
    archive
        .file_mut()?
        .flush()
        .map_err(|_| "archive_disk_write_failed")?;
    archive
        .file_mut()?
        .seek(SeekFrom::Start(0))
        .map_err(|_| "artifact_archive_seek_failed")?;
    let file = archive.take_file()?;
    extract_artifact_archive(
        file,
        bytes_written,
        max_archive_bytes,
        expectation,
        task,
        destination,
    )
}

struct TemporaryArchive {
    path: std::path::PathBuf,
    file: Option<File>,
}

impl TemporaryArchive {
    fn create(path: std::path::PathBuf) -> Result<Self, &'static str> {
        let fd = rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .map_err(|_| "artifact_archive_temp_create_failed")?;
        Ok(Self {
            path,
            file: Some(File::from(fd)),
        })
    }

    fn file_mut(&mut self) -> Result<&mut File, &'static str> {
        self.file.as_mut().ok_or("artifact_archive_temp_closed")
    }

    fn take_file(&mut self) -> Result<File, &'static str> {
        self.file.take().ok_or("artifact_archive_temp_closed")
    }
}

impl Drop for TemporaryArchive {
    fn drop(&mut self) {
        drop(self.file.take());
        drop(fs::remove_file(&self.path));
    }
}

#[cfg(test)]
#[path = "retrieve_artifact_build/tests.rs"]
mod tests;
