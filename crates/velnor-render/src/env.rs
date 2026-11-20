// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{StepKind, WorkflowPlan};

use crate::base_steps::roots_env;
use crate::yaml::{Yaml, entry, text};

pub(crate) fn workflow_env(plan: &WorkflowPlan) -> Yaml {
    let roots: BTreeMap<String, String> = plan
        .jobs
        .iter()
        .flat_map(|job| &job.steps)
        .filter_map(|step| match &step.kind {
            StepKind::SourcesFetchAll { manifests, .. } => roots_env(manifests),
            _ => None,
        })
        .collect();
    Yaml::Map(
        [
            entry("MISE_AUTO_INSTALL", text("false")),
            entry("RUSTUP_AUTO_INSTALL", text("0")),
        ]
        .into_iter()
        .chain(roots.iter().map(|(name, value)| entry(name, text(value))))
        .collect(),
    )
}
