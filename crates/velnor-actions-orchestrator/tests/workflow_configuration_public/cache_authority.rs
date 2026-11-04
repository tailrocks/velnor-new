// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, CacheAccess, CacheWriter, Job, WorkflowIr, WorkflowPolicy,
};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};

use super::maintenance_features::owner_feature_repo;
use super::tools_producers::{POLICIES, is_tools_producer};
use super::tools_producers_donors::SCHEDULED_SUITES_CONFIG;
use super::waste_oracle::{Files, files_of, files_of_repo};
use super::workflow_configuration_fixture;

const CI_PATH: &str = ".github/workflows/ci.yml";
const QUALIFICATION_PATH: &str = ".github/workflows/qualification.yml";
const SOURCES_PRODUCER: &str = "sources-producer";
const STATE_SEAL_WRITER: &str = "state-canary-seal";
const QUALIFICATION_WRITERS: [&str; 4] = [
    "cache-scale-set",
    "empty-cache-hosted",
    "empty-cache-scale-set",
    "mbx-cache-write-hosted",
];
const NON_DEFAULT_BRANCH: &str =
    "github.ref != format('refs/heads/{0}', github.event.repository.default_branch)";
const TRUSTED_PUSH: &str = "github.event_name == 'push'";
const PROTECTED_REF: &str = "github.ref_protected == true";
const DEFAULT_MODE: &str = "DEFAULT";
const COMMITTED_WORKFLOWS: [&str; 11] = [
    "cache-prune.yml",
    "ci.yml",
    "freshness.yml",
    "homebrew-adoption.yml",
    "job-image-candidate.yml",
    "product-release-binary.yml",
    "product-release-generator.yml",
    "product-release-images.yml",
    "product-release.yml",
    "qualification.yml",
    "source-qualification.yml",
];

struct JobModes {
    id: String,
    mode: Option<String>,
    condition: String,
}

struct FileModes {
    workflow: Vec<String>,
    jobs: Vec<JobModes>,
}

fn scalar(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

fn scan(text: &str) -> FileModes {
    let mut scanned = FileModes {
        workflow: Vec::new(),
        jobs: Vec::new(),
    };
    let mut in_jobs = false;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let content = line.trim();
        match indent {
            0 => {
                in_jobs = content == "jobs:";
                if let Some(value) = content.strip_prefix("cache-mode:") {
                    scanned.workflow.push(scalar(value));
                }
            }
            2 if in_jobs && content.ends_with(':') => scanned.jobs.push(JobModes {
                id: content.trim_end_matches(':').to_owned(),
                mode: None,
                condition: String::new(),
            }),
            4 if in_jobs => {
                if let Some(job) = scanned.jobs.last_mut() {
                    if let Some(value) = content.strip_prefix("cache-mode:") {
                        job.mode = Some(scalar(value));
                    } else if let Some(value) = content.strip_prefix("if:") {
                        job.condition = scalar(value);
                    }
                }
            }
            _ => {}
        }
    }
    scanned
}

fn resolves_write(file: &FileModes, job: &JobModes) -> bool {
    let mode = job
        .mode
        .as_deref()
        .or(file.workflow.first().map(String::as_str))
        .unwrap_or(DEFAULT_MODE);
    !matches!(mode, "read" | "none")
}

fn expected_writer(path: &str, id: &str) -> bool {
    if path == QUALIFICATION_PATH {
        return QUALIFICATION_WRITERS.contains(&id);
    }
    id.starts_with("tools-producer-")
        || id.starts_with("providers-producer-")
        || id == SOURCES_PRODUCER
        || id == STATE_SEAL_WRITER
}

fn workflow_files(files: &Files) -> impl Iterator<Item = (&String, &String)> {
    files.iter().filter(|(path, _)| {
        path.starts_with(".github/workflows/")
            && std::path::Path::new(path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("yml"))
    })
}

fn check_files(label: &str, files: &Files) -> BTreeMap<String, BTreeSet<String>> {
    let mut writers = BTreeMap::new();
    for (path, text) in workflow_files(files) {
        let file = scan(text);
        assert_eq!(
            file.workflow,
            ["read"],
            "{label}: {path} has no workflow-level cache-mode read"
        );
        assert!(!file.jobs.is_empty(), "{label}: {path} has no jobs");
        let mut actual = BTreeSet::new();
        for job in &file.jobs {
            let can_write = resolves_write(&file, job);
            assert_eq!(
                can_write,
                expected_writer(path, &job.id),
                "{label}: {path} job {} resolves write={can_write}",
                job.id
            );
            if can_write {
                actual.insert(job.id.clone());
                let gate = if path == QUALIFICATION_PATH {
                    vec![NON_DEFAULT_BRANCH]
                } else {
                    vec![TRUSTED_PUSH, PROTECTED_REF]
                };
                for needle in gate {
                    assert!(
                        job.condition.contains(needle),
                        "{label}: {path} writer {} lacks {needle}: {}",
                        job.id,
                        job.condition
                    );
                }
            }
        }
        writers.insert(path.clone(), actual);
    }
    writers
}

fn fixture(policy: WorkflowPolicy) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(policy, false, "default", "default")?;
    workflow_configuration_fixture::write_root_cargo_lock(repo.path(), &["demo", "maintenance"])?;
    Ok(repo)
}

fn finalized_with_sources(
    policy: WorkflowPolicy,
) -> Result<(WorkflowIr, BTreeMap<String, Job>), Box<dyn Error>> {
    let repo = fixture(policy)?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    Ok((prepared.workflow.ir.clone(), jobs))
}

fn rendered(policy: WorkflowPolicy) -> Result<Files, Box<dyn Error>> {
    let repo = fixture(policy)?;
    files_of(&render_staged_tree(&prepare(repo.path())?)?)
}

#[test]
fn every_generated_workflow_resolves_read_except_declared_writers() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let label = format!("{policy:?}");
        let files = rendered(policy)?;
        let writers = check_files(&label, &files);
        let ci = writers.get(CI_PATH).ok_or("ci.yml not rendered")?;
        assert!(
            ci.iter().any(|id| id.starts_with("tools-producer-")),
            "{label}: no tools producer writes: {ci:?}"
        );
        assert!(ci.contains(SOURCES_PRODUCER), "{label}: {ci:?}");
        assert!(workflow_files(&files).count() >= 2, "{label}");
    }
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let files = files_of(&render_staged_tree(&prepare(repo.path())?)?)?;
    let writers = check_files("suites", &files);
    assert!(
        writers.values().any(|ids| !ids.is_empty()),
        "suites: no writer rendered"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let committed = files_of_repo(&root)?;
    let writers = check_files("committed", &committed);
    for name in COMMITTED_WORKFLOWS {
        assert!(
            writers.contains_key(&format!(".github/workflows/{name}")),
            "committed tree lost {name}"
        );
    }
    assert_eq!(
        writers.get(QUALIFICATION_PATH),
        Some(
            &QUALIFICATION_WRITERS
                .iter()
                .map(|id| (*id).to_owned())
                .collect()
        )
    );
    let ci = writers.get(CI_PATH).ok_or("committed ci.yml missing")?;
    assert!(ci.contains(SOURCES_PRODUCER), "{ci:?}");
    Ok(())
}

fn validated(ir: &WorkflowIr, jobs: &BTreeMap<String, Job>) -> Result<(), String> {
    let mut candidate = ir.clone();
    candidate.jobs.clone_from(jobs);
    candidate.validate().map_err(|error| error.to_string())
}

fn rejected(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    id: &str,
    access: CacheAccess,
    rule: &str,
) -> Result<(), Box<dyn Error>> {
    let mut mutated = jobs.clone();
    mutated.get_mut(id).ok_or("job missing")?.cache_access = access;
    let error = validated(ir, &mutated)
        .err()
        .ok_or_else(|| format!("{id} accepted {access:?}"))?;
    assert!(
        error.contains(&format!("{rule}:{id}")),
        "{id} {access:?}: {error}"
    );
    Ok(())
}

#[test]
fn a_writer_role_on_a_job_that_runs_task_code_fails_generation() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (ir, jobs) = finalized_with_sources(policy)?;
        validated(&ir, &jobs)?;
        let crate_id = jobs
            .keys()
            .find(|id| id.starts_with(CRATE_JOB_ID_PREFIX))
            .ok_or("no crate job")?;
        for writer in [CacheWriter::ToolsProducer, CacheWriter::SourcesProducer] {
            rejected(
                &ir,
                &jobs,
                crate_id,
                CacheAccess::Writer(writer),
                "cache_writer_runs_task_code",
            )?;
        }
        rejected(
            &ir,
            &jobs,
            "jactionlint",
            CacheAccess::Writer(CacheWriter::ToolsProducer),
            "cache_writer_not_a_producer",
        )?;
        let producer = jobs
            .iter()
            .find(|(_, job)| is_tools_producer(job))
            .map(|(id, _)| id.clone())
            .ok_or("no tools producer")?;
        rejected(
            &ir,
            &jobs,
            &producer,
            CacheAccess::Writer(CacheWriter::SourcesProducer),
            "cache_writer_not_a_producer",
        )?;
        rejected(
            &ir,
            &jobs,
            &producer,
            CacheAccess::Read,
            "cache_writer_producer_without_write",
        )?;
    }
    Ok(())
}
