// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{CargoCommand, RepoPath, RootSlug, StepKind, WorkflowPlan};
use velnor_rust::{PackageCounts, encode_package_counts};

use crate::pipeline::{HELPER_LOCK, view};

pub(crate) type Packages = BTreeMap<String, Vec<String>>;

pub(crate) fn saving_facts(
    roots: usize,
    blocks: bool,
) -> Result<velnor_model::RepoFacts, Box<dyn std::error::Error>> {
    let manifests: Vec<_> = (0..roots)
        .map(|index| format!("root_{index:02}_{}/Cargo.toml", "x".repeat(150)))
        .collect();

    let mut view = view(
        &manifests.iter().map(String::as_str).collect::<Vec<_>>(),
        Some(format!("shape = \"lint-test\"\ncache_check_blocks = {blocks}\n").as_bytes()),
    )?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.79", "0.1.81", 1);
    view.files
        .insert(RepoPath::new(".velnor/rust-ci.lock")?, lock.into_bytes());
    Ok(velnor_rust::detect(&view)?)
}

pub(crate) fn saving_plan(
    roots: usize,
    blocks: bool,
) -> Result<WorkflowPlan, Box<dyn std::error::Error>> {
    Ok(velnor_rust::plan(&saving_facts(roots, blocks)?))
}

pub(crate) fn test_packages(
    roots: usize,
    blocks: bool,
) -> Result<Packages, Box<dyn std::error::Error>> {
    let facts = saving_facts(roots, blocks)?;
    let plan = velnor_rust::plan(&facts);
    let mut packages = Packages::new();
    for job in &plan.jobs {
        let slug = job.steps.iter().find_map(|step| match &step.kind {
            StepKind::Cargo(
                CargoCommand::NextestRun { .. } | CargoCommand::NextestRunArchive { .. },
            ) => step.reads.as_ref().map(|plan| plan.slug.clone()),
            _ => None,
        });
        let Some(slug) = slug else {
            continue;
        };
        let root = facts
            .rust_roots
            .iter()
            .find(|root| RootSlug::of(&root.manifest) == slug)
            .ok_or("test root")?;
        packages.insert(
            job.id.as_str().to_owned(),
            root.members
                .iter()
                .map(|member| member.name.as_str().to_owned())
                .collect(),
        );
    }
    Ok(packages)
}

pub(crate) fn worst_package_counts(
    members: &[String],
) -> Result<String, Box<dyn std::error::Error>> {
    let counts = members
        .iter()
        .map(|member| {
            (
                member.clone(),
                PackageCounts {
                    run: u64::MAX,
                    listed: u64::MAX,
                },
            )
        })
        .collect();
    encode_package_counts(&counts).ok_or_else(|| "package counts".into())
}
