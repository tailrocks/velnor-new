// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Job, RootSlug, StepKind};

use crate::cargo::reports_tests;

pub(crate) const TEST_STEP_ID: &str = "test";

#[derive(Clone, Debug, Default)]
pub(crate) struct Tests {
    lanes: Vec<RootSlug>,
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
        Self { lanes }
    }

    pub(crate) fn id(&self, slug: Option<&RootSlug>) -> String {
        match (self.lanes.len() > 1, slug) {
            (true, Some(slug)) => format!("{TEST_STEP_ID}_{slug}"),
            _ => TEST_STEP_ID.to_owned(),
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
                        format!("${{{{ steps.{TEST_STEP_ID}_{slug}.outputs.{name} }}}}"),
                    )
                })
                .collect();
        }
        vec![(
            name.to_owned(),
            format!("${{{{ steps.{TEST_STEP_ID}.outputs.{name} }}}}"),
        )]
    }
}
