//! Generated Gradle jobs: one per project (hard decision 5).
//!
//! `[stacks.gradle]` names the roots. Generation analyses each root statically
//! (`velnor-actions-gradle`) and fails closed with the adapter's finding codes
//! when a root violates the stack contract. Each project with a build script
//! gets a job: checkout, helper, the private-source block for the lock's Maven
//! bundles, Mise, then the `gradle-run-v1` step, which verifies the wrapper and
//! runs `assemble` and `check` as one `--continue` invocation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::config::gradle::{GRADLE_JOB_PREFIX, GradleService};
use velnor_actions_contract::{
    GRADLE_RUN_OPERATION, Job, JobTimeout, Permissions, ServiceContainer, Step, StepKind,
    VelnorConfig, slugify_segment,
};
use velnor_actions_gradle::analysis::analyze_root;
use velnor_actions_workflow_renderer::mise_setup_step;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use crate::OrchestratorError;
use crate::config::gradle_run::{DATABASE_ENV, JAVA_ENV, ROOT_ENV, TASKS_ENV, WORKERS_ENV};
use crate::config::private_wiring::PrivateWiring;
use crate::safe_read::{RepoBytes, read_repo_bytes};

/// Largest Gradle file read (verification metadata can be megabytes).
const MAX_GRADLE_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// One analysed Gradle root: its directory and the projects with build scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GradleRoot {
    pub(crate) root: String,
    pub(crate) projects: Vec<String>,
}

fn join(root: &str, rel: &str) -> String {
    if root.is_empty() || root == "." {
        rel.to_owned()
    } else {
        format!("{root}/{rel}")
    }
}

/// Analyse every configured root; fail closed on any finding.
///
/// # Errors
/// A contract error listing `<root>: <finding>` codes.
pub(crate) fn analyze_roots(
    repo: &Path,
    index: &velnor_actions_contract::FileIndex,
    config: &VelnorConfig,
) -> Result<Vec<GradleRoot>, OrchestratorError> {
    let Some(config) = config.stacks.gradle.as_ref() else {
        return Ok(Vec::new());
    };
    let files = index.files();
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for dir in &config.roots {
        let root = if dir.as_str() == "." {
            ""
        } else {
            dir.as_str()
        };
        let prefix = if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        };
        let rel: BTreeSet<String> = files
            .iter()
            .filter_map(|file| file.strip_prefix(&prefix).map(str::to_owned))
            .collect();
        let read = |path: &str| -> Option<Vec<u8>> {
            match read_repo_bytes(repo, &join(root, path), MAX_GRADLE_FILE_BYTES) {
                Ok(RepoBytes::Bytes(bytes)) => Some(bytes),
                _ => None,
            }
        };
        let analysis = analyze_root(root, &rel, &read);
        problems.extend(
            analysis
                .findings
                .iter()
                .map(|code| format!("{dir:?}: {code}", dir = dir.as_str()).replace('"', "")),
        );
        let Some(model) = &analysis.settings else {
            continue;
        };
        let projects = model
            .projects
            .iter()
            .filter(|project| {
                ["build.gradle.kts", "build.gradle"]
                    .iter()
                    .any(|name| rel.contains(&format!("{}/{name}", project.dir)))
            })
            .map(|project| project.path.clone())
            .collect();
        found.push(GradleRoot {
            root: root.to_owned(),
            projects,
        });
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(OrchestratorError::Contract {
            problem: format!("gradle_stack_findings:{}", problems.join("; ")),
        })
    }
}

/// Job id for one project.
fn job_id(root: &str, project: &str) -> String {
    let scope = if root.is_empty() {
        String::new()
    } else {
        format!("{}-", slugify_segment(root))
    };
    format!(
        "{GRADLE_JOB_PREFIX}{scope}{}",
        slugify_segment(project.trim_start_matches(':'))
    )
}

/// The `gradle-run-v1` step of one project.
fn run_step(root: &str, project: &str, workers: u8, database: bool) -> Step {
    Step {
        parallel: false,
        name: "Gradle assemble and check".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: GRADLE_RUN_OPERATION.to_owned(),
            env: BTreeMap::from([
                (
                    ROOT_ENV.to_owned(),
                    if root.is_empty() {
                        ".".to_owned()
                    } else {
                        root.to_owned()
                    },
                ),
                (
                    TASKS_ENV.to_owned(),
                    format!("{project}:assemble {project}:check"),
                ),
                (WORKERS_ENV.to_owned(), workers.to_string()),
                (
                    DATABASE_ENV.to_owned(),
                    if database { "1" } else { "0" }.to_owned(),
                ),
                (JAVA_ENV.to_owned(), format!("${{{JAVA_ENV}}}")),
            ]),
        },
    }
}

/// Build every Gradle job.
///
/// # Errors
/// Missing helper (non-consumer policy), pin and private-source errors.
pub(crate) fn build_jobs(
    config: &VelnorConfig,
    roots: &[GradleRoot],
    acquire: Option<&Step>,
    private: Option<&PrivateWiring>,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let Some(settings) = &config.stacks.gradle else {
        return Ok(Vec::new());
    };
    let acquire = acquire.ok_or_else(|| OrchestratorError::Contract {
        problem: "gradle_requires_consumer_policy".to_owned(),
    })?;
    let setup = crate::pins::resolve_mise_setup_for_release_target(
        config,
        velnor_actions_contract::ReleaseTarget::LinuxX86_64,
    )?;
    let minutes = settings
        .timeout_minutes
        .map_or(Ok(config.workflow.crate_timeout_minutes), u16::try_from)
        .map_err(|_| OrchestratorError::Contract {
            problem: "gradle_timeout_range".to_owned(),
        })?;
    let workers = settings.max_workers.unwrap_or(4);
    let mut jobs = Vec::new();
    for root in roots {
        let mut prefix = vec![acquire.clone()];
        if let Some(private) = private {
            prefix.extend(private.steps_for_gradle(std::slice::from_ref(&root.root))?);
        }
        for project in &root.projects {
            let database = settings
                .projects
                .get(project)
                .is_some_and(|p| p.services.contains(&GradleService::Postgres));
            let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
            steps.extend(prefix.iter().cloned());
            steps.push(mise_setup_step(&setup).map_err(OrchestratorError::from)?);
            steps.push(run_step(&root.root, project, workers, database));
            let services = if database {
                vec![
                    ServiceContainer::postgres(18).map_err(|err| OrchestratorError::Contract {
                        problem: err.to_string(),
                    })?,
                ]
            } else {
                Vec::new()
            };
            jobs.push((
                job_id(&root.root, project),
                Job {
                    services,
                    check_runner: None,
                    display_name: format!("Gradle / {}", project.trim_start_matches(':')),
                    runs_on: config.workflow.runner_label.clone().unwrap_or_else(|| {
                        velnor_actions_contract::config::LATEST_RUNNER_LABEL.to_owned()
                    }),
                    timeout_minutes: JobTimeout::new(minutes)?,
                    needs: vec![PLAN_JOB_ID.to_owned()],
                    condition: None,
                    permissions: Some(Permissions::default()),
                    environment: None,
                    steps,
                    cache_mode: None,
                },
            ));
        }
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(jobs)
}

#[cfg(test)]
#[path = "gradle_jobs_tests.rs"]
mod tests;
