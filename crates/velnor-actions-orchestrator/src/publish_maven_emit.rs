// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::ServiceContainer;
use velnor_actions_contract::config::gradle::GradleService;
use velnor_actions_contract::config::{PublishMaven, VersionSource};
use velnor_actions_workflow_renderer::publish_spec::PublishStep;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::config::gradle_jobs::run_step_for;
use crate::config::private_wiring::PrivateWiring;
use crate::prepare::GenerationPreparation;
use crate::publish_steps::{MavenPlan, VersionCheck};
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

fn failure(index: usize, field: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::config(
        CONFIG_REL,
        format!("publish.maven[{index}].{field}"),
        problem,
    )
}

pub(crate) fn resolve<'a>(
    prep: &GenerationPreparation,
    private: Option<&PrivateWiring>,
    index: usize,
    entry: &'a PublishMaven,
) -> Result<MavenPlan<'a>, OrchestratorError> {
    let root = if entry.root == "." {
        ""
    } else {
        entry.root.as_str()
    };
    let gradle = prep
        .config
        .stacks
        .gradle
        .as_ref()
        .ok_or_else(|| failure(index, "root", "not_a_stacks_gradle_root"))?;
    if !prep
        .discovery
        .gradle_roots
        .iter()
        .any(|analysed| analysed.root == root)
    {
        return Err(failure(index, "root", "not_a_stacks_gradle_root"));
    }
    let version_file = entry.version_file();
    let text = match read_repo_file(&prep.root, &version_file, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(text) => text,
        RepoRead::Absent => {
            return Err(failure(
                index,
                "root",
                &format!("missing_file:{version_file}"),
            ));
        }
    };
    if VersionSource::GradleProperties.read(&text).is_none() {
        return Err(failure(
            index,
            "root",
            &format!("version_not_literal:{version_file}"),
        ));
    }
    let acquire = crate::pins::consumer_acquire_step(
        &prep.runner_label,
        env!("CARGO_PKG_VERSION"),
        &prep.discovery,
    )?;
    let mut prefix = vec![PublishStep {
        id: None,
        step: acquire,
    }];
    if let Some(private) = private {
        prefix.extend(
            private
                .steps_for_gradle(&[root.to_owned()])?
                .into_iter()
                .map(|step| PublishStep {
                    id: step.id.map(|id| id.as_str().to_owned()),
                    step,
                }),
        );
    }
    let minutes = gradle
        .timeout_minutes
        .map_or(
            Ok(prep.config.workflow.crate_timeout_minutes),
            u16::try_from,
        )
        .map_err(|_| failure(index, "root", "gradle_timeout_range"))?;
    let services = root_services(prep, gradle, root)?;
    let database = !services.is_empty();
    Ok(MavenPlan {
        entry,
        check: VersionCheck::Manifest(VersionSource::GradleProperties, version_file),
        prefix,
        gradle: run_step_for(
            "Gradle mavenBundle",
            root,
            velnor_actions_contract::config::MAVEN_BUNDLE_TASK,
            gradle.max_workers.unwrap_or(4),
            database,
        ),
        timeout: JobTimeout::new(minutes)?,
        services,
    })
}

fn root_services(
    prep: &GenerationPreparation,
    gradle: &velnor_actions_contract::config::gradle::GradleStackConfig,
    root: &str,
) -> Result<Vec<ServiceContainer>, OrchestratorError> {
    let database = prep.discovery.gradle_roots.iter().any(|analysed| {
        analysed.root == root
            && analysed.projects.iter().any(|project| {
                gradle
                    .projects
                    .get(project)
                    .is_some_and(|p| p.services.contains(&GradleService::Postgres))
            })
    });
    if !database {
        return Ok(Vec::new());
    }
    let postgres = ServiceContainer::postgres(18).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    Ok(vec![postgres])
}
