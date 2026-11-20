// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, Job, RootSlug, StepKind, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT};

use crate::cargo::reports_tests;

pub(crate) const TEST_STEP_ID: &str = "test";

#[derive(Clone, Debug, Default)]
pub(crate) struct Tests {
    lanes: Vec<RootSlug>,
    skippable: bool,
}

impl Tests {
    pub(crate) fn of(job: &Job) -> Self {
        let mut lanes: Vec<RootSlug> = Vec::new();
        for step in &job.steps {
            let StepKind::Cargo(command) = &step.kind else {
                continue;
            };
            if !reports_tests(command) {
                continue;
            }
            if let Some(plan) = &step.reads
                && !lanes.contains(&plan.slug)
            {
                lanes.push(plan.slug.clone());
            }
        }
        let skippable = job.steps.iter().any(|step| {
            matches!(
                &step.kind,
                StepKind::Cargo(CargoCommand::NextestFeatures { packages, reports: true, .. })
                    if !packages.is_empty()
            )
        });
        Self { lanes, skippable }
    }

    fn read(&self, id: &str, name: &str) -> String {
        let output = format!("steps.{id}.outputs.{name}");
        if self.skippable && [TESTS_RUN_OUTPUT, TESTS_LISTED_OUTPUT].contains(&name) {
            format!("${{{{ {output} || '0' }}}}")
        } else {
            format!("${{{{ {output} }}}}")
        }
    }

    pub(crate) fn id(&self, slug: Option<&RootSlug>) -> String {
        match (self.lanes.len() > 1, slug) {
            (true, Some(slug)) => format!("{TEST_STEP_ID}_{slug}"),
            _ => TEST_STEP_ID.to_owned(),
        }
    }

    /// Whether a test step of the job failed, as a `${{ }}` body that stands in an `&&` chain; none without test steps.
    pub(crate) fn failed(&self) -> Option<String> {
        let ids = self
            .lanes
            .iter()
            .map(|slug| format!("steps.{}.outcome == 'failure'", self.id(Some(slug))))
            .collect::<Vec<_>>();
        match ids.as_slice() {
            [] => None,
            [only] => Some(only.clone()),
            _ => Some(format!("({})", ids.join(" || "))),
        }
    }

    pub(crate) fn first_id(&self) -> Option<String> {
        self.lanes.first().map(|slug| self.id(Some(slug)))
    }

    pub(crate) fn outputs(&self, name: &str) -> Vec<(String, String)> {
        if self.lanes.len() > 1 {
            return self
                .lanes
                .iter()
                .map(|slug| {
                    (
                        format!("{name}_{slug}"),
                        self.read(&format!("{TEST_STEP_ID}_{slug}"), name),
                    )
                })
                .collect();
        }
        vec![(name.to_owned(), self.read(TEST_STEP_ID, name))]
    }
}
