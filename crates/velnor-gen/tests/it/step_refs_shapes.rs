// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_model::{RepoPath, WORKFLOWS_CONFIG_PATH};

use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::view;
use crate::public_sources::{DECLARED as PUBLIC_SOURCES, SHA, SOURCE};
use crate::release_family_fixture;
use crate::release_fixture::release_view;
use crate::step_refs_sweep::{
    Case, Sweep, gate_patch, gate_patches, generated, pinned, rust_ci_sweep,
};

const DECLARED: &str = "[[workflow]]\nfile = \"declared\"\nname = \"Declared\"\n\n[[workflow.job]]\nid = \"check\"\n\n[[workflow.job.step]]\nid = \"warm\"\nname = \"Warm the cache\"\nrun = \"echo ok\"\n\n[[workflow.job.step]]\nname = \"Use the warm step\"\nrun = \"echo $WARM\"\nenv = { WARM = \"${{ steps.warm.outputs.ready }}\" }\n";

const FAMILY_KEYS: [&str; 5] = [
    "roots",
    "family_tag",
    "binaries.package",
    "binaries.smoke",
    "binaries.platforms",
];

fn family_first() -> Result<u64, Box<dyn Error>> {
    let mut first = 0;
    for key in FAMILY_KEYS {
        first = first.max(gate_patch(key)?);
    }
    Ok(first)
}

#[test]
fn every_steps_reference_in_a_declared_workflow_names_a_step_of_its_own_job() -> TestResult {
    let mut sweep = Sweep::default();
    for patch in gate_patches()? {
        let mut repo = release_view(&[], None)?;
        repo.files
            .insert(WORKFLOWS_CONFIG_PATH, DECLARED.as_bytes().to_vec());
        let effective = pinned(&mut repo, patch)?;
        sweep.add(
            &format!("0.1.{effective}, declared"),
            &generated(&mut repo)?,
        )?;
    }
    sweep.finish();
    Ok(())
}

const TAG_PUSH: &str = "family_trigger = \"tag_push\"\ncommit_file = true\n";

#[test]
fn every_steps_reference_in_a_family_release_workflow_names_a_step_of_its_own_job() -> TestResult {
    let newest = *gate_patches()?
        .last()
        .ok_or("the gate scan found no helper")?;
    let tag_push = family_first()?
        .max(gate_patch("family_trigger")?)
        .max(gate_patch("commit_file")?);
    let mut sweep = Sweep::default();
    for (first, config, label) in [
        (
            family_first()?,
            release_family_fixture::tools_like(""),
            "family",
        ),
        (
            tag_push,
            release_family_fixture::tools_like(TAG_PUSH),
            "tag-push family",
        ),
    ] {
        for patch in first..=newest {
            let helper = format!("0.1.{patch}");
            let (text, _) = release_family_fixture::generate(&helper, &config)?;
            let files = BTreeMap::from([(RepoPath::new(".github/workflows/family.yml")?, text)]);
            sweep.add(&format!("{helper}, {label}"), &files)?;
        }
    }
    sweep.finish();
    Ok(())
}

#[test]
fn every_steps_reference_in_a_public_sources_job_names_a_step_of_its_own_job() -> TestResult {
    let public_sources = gate_patch("public_sources")?;
    let lock = format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dep0\"\nversion = \"1.0.0\"\nsource = \"git+{SOURCE}?rev=v1#{SHA}\"\n"
    );
    rust_ci_sweep(&[Case {
        from: public_sources,
        config: Some(PUBLIC_SOURCES),
        files: &[
            ("Cargo.lock", lock.as_str()),
            ("tools/Cargo.lock", lock.as_str()),
        ],
    }])
}

const PARALLEL_LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\njob = 'parallel'\n";

#[test]
fn every_steps_reference_in_a_workspace_group_job_names_a_step_of_its_own_job() -> TestResult {
    rust_ci_sweep(&[Case {
        from: gate_patch("workspace_groups")?,
        config: Some("[[workspace_groups]]\nname = 'pair'\nroots = ['.', 'tools']\n"),
        files: &[],
    }])
}

#[test]
fn every_steps_reference_in_a_lint_test_shape_job_names_a_step_of_its_own_job() -> TestResult {
    rust_ci_sweep(&[Case {
        from: 0,
        config: Some("shape = 'lint-test'\n"),
        files: &[],
    }])
}

#[test]
fn every_steps_reference_in_a_parallel_test_lane_job_names_a_step_of_its_own_job() -> TestResult {
    rust_ci_sweep(&[Case {
        from: gate_patch("test_lane.job")?,
        config: Some(PARALLEL_LANE),
        files: &[],
    }])
}

#[test]
fn every_steps_reference_in_an_mbx_remote_job_names_a_step_of_its_own_job() -> TestResult {
    rust_ci_sweep(&[Case {
        from: gate_patch("cache.mbx_remote")?,
        config: Some("[cache]\nmbx_remote = \"github-actions\"\n"),
        files: &[],
    }])
}

#[test]
fn every_steps_reference_in_a_dependency_policy_job_names_a_step_of_its_own_job() -> TestResult {
    let policy = gate_patch("policy.deny")?;
    let mut sweep = Sweep::default();
    for patch in gate_patches()?.into_iter().filter(|patch| *patch >= policy) {
        let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
        for (path, text) in [
            ("deny.toml", ""),
            ("mise.toml", DENY_MISE),
            (".velnor/rust-ci.toml", "[policy]\ndeny = true\n"),
        ] {
            let path = RepoPath::new(path)?;
            repo.tracked_paths.insert(path.clone());
            repo.files.insert(path, text.as_bytes().to_vec());
        }
        let effective = pinned(&mut repo, patch)?;
        sweep.add(&format!("0.1.{effective}, deny"), &generated(&mut repo)?)?;
    }
    sweep.finish();
    Ok(())
}
