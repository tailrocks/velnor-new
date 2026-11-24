// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{StepKind, WorkflowPlan};

use crate::yaml::{Yaml, entry, text};

pub(crate) fn workflow_env(plan: &WorkflowPlan, roots: &BTreeMap<String, String>) -> Yaml {
    let mut pairs = vec![entry("MISE_AUTO_INSTALL", text("false"))];
    if installs_rust(plan) {
        pairs.push(entry("RUSTUP_AUTO_INSTALL", text("0")));
    }
    Yaml::Map(
        pairs
            .into_iter()
            .chain(roots.iter().map(|(name, value)| entry(name, text(value))))
            .collect(),
    )
}

fn installs_rust(plan: &WorkflowPlan) -> bool {
    plan.jobs.iter().flat_map(|job| &job.steps).any(|step| {
        matches!(
            step.kind,
            StepKind::Cargo(_)
                | StepKind::RustComponents(_)
                | StepKind::RustTargets(_)
                | StepKind::MbxRemote { .. }
                | StepKind::MbxPin
                | StepKind::MbxPrivate
                | StepKind::MbxOutDirs
                | StepKind::MbxStore
                | StepKind::MbxSeed
        )
    })
}
