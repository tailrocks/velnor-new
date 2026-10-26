// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    LogicalJobFact, LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, SuitePhase,
    invalid, seeds, validate_root,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct RegisteredSources {
    seed_ids: BTreeMap<String, String>,
    seen_seeds: BTreeSet<String>,
    gradle: BTreeSet<(String, String)>,
    seen_gradle: BTreeSet<(String, String)>,
    seen_suites: BTreeSet<String>,
    generated: BTreeMap<String, super::generated::GeneratedOwner>,
    seen_generated: BTreeSet<String>,
    seen_workflow: BTreeSet<String>,
}

impl RegisteredSources {
    pub(super) fn new(input: &NativeAuthorityInput<'_>) -> Result<Self, NativeAuthorityError> {
        let roots = seeds::roots(input)?;
        let seed_ids = seeds::unique_dependency_seed_job_ids(&roots)
            .into_iter()
            .zip(roots)
            .map(|(id, root)| (root, id))
            .collect();
        let gradle = gradle_projects(input)?;
        Ok(Self {
            generated: super::generated::inventory(input)?,
            seen_generated: BTreeSet::new(),
            seed_ids,
            seen_seeds: BTreeSet::new(),
            gradle,
            seen_gradle: BTreeSet::new(),
            seen_suites: BTreeSet::new(),
            seen_workflow: BTreeSet::new(),
        })
    }

    pub(super) fn admit(
        &mut self,
        fact: &LogicalJobFact,
        input: &NativeAuthorityInput<'_>,
    ) -> Result<bool, NativeAuthorityError> {
        match &fact.source {
            LogicalJobSource::NamedCheck { id } => {
                let runner = input
                    .config
                    .checks
                    .iter()
                    .find(|check| check.id == *id)
                    .map(|check| &check.runner)
                    .or_else(|| {
                        input
                            .config
                            .native_checks
                            .iter()
                            .find(|check| check.id == *id)
                            .map(|check| &check.runner)
                    });
                if fact.id != format!("check-{id}")
                    || runner.is_none_or(|runner| {
                        fact.check_runner.as_ref() != Some(runner) || fact.runs_on != runner.label
                    })
                {
                    return Err(invalid("native_check_job_identity"));
                }
                Ok(true)
            }
            LogicalJobSource::Gradle { root, project } => self.gradle(fact, root, project),
            LogicalJobSource::Suite { id, phase } => self.suite(fact, id, *phase, input),
            LogicalJobSource::WorkflowTask { owner } => {
                let configured = input.config.workflow.tasks.iter().any(|task| {
                    task.id() == owner
                        && fact.id
                            == format!(
                                "{}{}",
                                velnor_actions_contract::WORKFLOW_TASK_JOB_PREFIX,
                                owner
                            )
                }) || input.config.images.iter().any(|image| {
                    image.id == *owner
                        && image
                            .platforms
                            .iter()
                            .any(|platform| image.build_job_id(*platform) == fact.id)
                });
                if !configured {
                    return Err(invalid("native_workflow_task_identity"));
                }
                self.seen_workflow.insert(fact.id.clone());
                Ok(false)
            }
            LogicalJobSource::GeneratedWorkflowTask { owner, kind } => {
                let source = self
                    .generated
                    .get(owner)
                    .ok_or_else(|| invalid("native_generated_owner_unknown"))?;
                if source.kind != *kind
                    || !source.jobs.contains(&fact.id)
                    || !self.seen_generated.insert(fact.id.clone())
                {
                    return Err(invalid("native_generated_task_identity"));
                }
                Ok(false)
            }
            LogicalJobSource::DependencySeed { root } => {
                if !self.seen_seeds.insert(root.clone())
                    || self.seed_ids.get(root) != Some(&fact.id)
                {
                    return Err(invalid("native_seed_job_identity"));
                }
                Ok(false)
            }
            LogicalJobSource::Task(_) => Err(invalid("native_registered_task_source")),
        }
    }

    fn gradle(
        &mut self,
        fact: &LogicalJobFact,
        root: &str,
        project: &str,
    ) -> Result<bool, NativeAuthorityError> {
        validate_root(root)?;
        let key = (root.to_owned(), project.to_owned());
        if !self.gradle.contains(&key) || !self.seen_gradle.insert(key) {
            return Err(invalid("native_gradle_project_membership"));
        }
        let scope = if root.is_empty() {
            String::new()
        } else {
            format!("{}-", velnor_actions_contract::slugify_segment(root))
        };
        let expected = format!(
            "{}{}{}",
            velnor_actions_contract::config::gradle::GRADLE_JOB_PREFIX,
            scope,
            velnor_actions_contract::slugify_segment(project.trim_start_matches(':'))
        );
        if fact.id != expected {
            return Err(invalid("native_gradle_job_identity"));
        }
        Ok(true)
    }

    fn suite(
        &mut self,
        fact: &LogicalJobFact,
        id: &str,
        phase: SuitePhase,
        input: &NativeAuthorityInput<'_>,
    ) -> Result<bool, NativeAuthorityError> {
        let suite = input
            .config
            .stacks
            .rust
            .as_ref()
            .and_then(|rust| rust.suites.iter().find(|suite| suite.id == id))
            .ok_or_else(|| invalid("native_suite_unknown"))?;
        let expected = match phase {
            SuitePhase::Build => format!("suite-build-{id}"),
            SuitePhase::Run => format!("suite-{id}"),
        };
        self.seen_suites.insert(fact.id.clone());
        if fact.id != expected || !suite.has_pr_or_push_trigger() {
            return Err(invalid("native_suite_job_identity"));
        }
        Ok(suite.required)
    }

    pub(super) fn finish(
        self,
        input: &NativeAuthorityInput<'_>,
    ) -> Result<(), NativeAuthorityError> {
        if self.seen_seeds != self.seed_ids.keys().cloned().collect()
            || self.seen_gradle != self.gradle
        {
            return Err(invalid("native_graph_incomplete_sources"));
        }
        if self.seen_generated
            != self
                .generated
                .values()
                .flat_map(|owner| owner.jobs.iter().cloned())
                .collect()
        {
            return Err(invalid("native_generated_inventory_incomplete"));
        }
        let mut suites = BTreeSet::new();
        if let Some(rust) = &input.config.stacks.rust {
            for suite in rust
                .suites
                .iter()
                .filter(|suite| suite.has_pr_or_push_trigger())
            {
                suites.insert(format!("suite-{}", suite.id));
                if suite.partitions > 1
                    && suite.build == velnor_actions_contract::RustSuiteBuildMode::Archive
                {
                    suites.insert(format!("suite-build-{}", suite.id));
                }
            }
        }
        if self.seen_suites != suites {
            return Err(invalid("native_graph_incomplete_suites"));
        }
        let mut workflow = input
            .config
            .workflow
            .tasks
            .iter()
            .map(|task| {
                format!(
                    "{}{}",
                    velnor_actions_contract::WORKFLOW_TASK_JOB_PREFIX,
                    task.id()
                )
            })
            .collect::<BTreeSet<_>>();
        workflow.extend(input.config.images.iter().flat_map(|image| {
            image
                .platforms
                .iter()
                .map(|platform| image.build_job_id(*platform))
        }));
        if self.seen_workflow != workflow {
            return Err(invalid("native_graph_incomplete_workflow_tasks"));
        }
        Ok(())
    }
}

fn gradle_projects(
    input: &NativeAuthorityInput<'_>,
) -> Result<BTreeSet<(String, String)>, NativeAuthorityError> {
    let configured = input
        .config
        .stacks
        .gradle
        .as_ref()
        .map(|settings| {
            settings
                .roots
                .iter()
                .map(|root| {
                    if root.as_str() == "." {
                        ""
                    } else {
                        root.as_str()
                    }
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    if configured
        != input
            .gradle_roots
            .iter()
            .map(|root| root.root.as_str())
            .collect()
    {
        return Err(invalid("native_gradle_root_membership"));
    }
    let mut projects = BTreeSet::new();
    for root in input.gradle_roots {
        validate_root(&root.root)?;
        if input.config.stacks.gradle.is_none() {
            return Err(invalid("native_gradle_without_config"));
        }
        for project in &root.projects {
            if !projects.insert((root.root.clone(), project.clone())) {
                return Err(invalid("native_gradle_duplicate_project"));
            }
        }
    }
    Ok(projects)
}
