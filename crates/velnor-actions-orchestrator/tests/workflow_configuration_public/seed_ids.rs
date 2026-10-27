// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;
use std::{fs, io};

use velnor_actions_contract::{Job, REQUIRED_JOB_ID, StepKind, StepRole, validate_job_id};
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::logical_owner_id;
use super::workflow_configuration_fixture;

#[test]
fn prepare_and_render_keep_distinct_seed_ids_for_colliding_nested_roots()
-> Result<(), Box<dyn Error>> {
    const ROOT_ENV: &str = "VELNOR_MBX_BUNDLE_ROOT";
    let expected_roots = [("crates/a-b", "a-b"), ("crates/a/b", "b")];
    let config = workflow_configuration_fixture::config_with_verification_branch(None);
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        format!(
            "[tools]\nrust = \"{}\"\n",
            velnor_actions_mise::RUST_VERSION
        ),
    )?;
    for (relative_root, package_name) in expected_roots {
        workflow_configuration_fixture::add_independent_mbx_package(
            root,
            relative_root,
            package_name,
        )?;
    }

    let prepared = prepare(root)?;
    assert_discovered_workspaces(&prepared, root, &expected_roots)?;
    let seed_ids = seed_ids_by_root(&prepared, &expected_roots, ROOT_ENV)?;
    assert_rendered_seed_routing(&prepared, &expected_roots, &seed_ids, ROOT_ENV)?;
    Ok(())
}

#[test]
fn prepare_and_render_keep_valid_seed_ids_for_dotted_and_hyphenated_roots()
-> Result<(), Box<dyn Error>> {
    const ROOT_ENV: &str = "VELNOR_MBX_BUNDLE_ROOT";
    let expected_roots = [
        ("crates/lib.v2", "dot-root"),
        ("crates/lib-v2", "hyphen-root"),
    ];
    let config = workflow_configuration_fixture::config_with_verification_branch(None);
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        format!(
            "[tools]\nrust = \"{}\"\n",
            velnor_actions_mise::RUST_VERSION
        ),
    )?;
    for (relative_root, package_name) in expected_roots {
        workflow_configuration_fixture::add_independent_mbx_package(
            root,
            relative_root,
            package_name,
        )?;
    }

    let prepared = prepare(root).map_err(|error| {
        io::Error::other(format!("dotted-root workflow generation failed: {error}"))
    })?;
    assert_discovered_workspaces(&prepared, root, &expected_roots)?;
    let seed_ids = seed_ids_by_root(&prepared, &expected_roots, ROOT_ENV)?;
    assert_rendered_seed_routing(&prepared, &expected_roots, &seed_ids, ROOT_ENV)?;
    Ok(())
}

fn assert_discovered_workspaces(
    prepared: &GenerationPreparation,
    root: &Path,
    expected_roots: &[(&str, &str)],
) -> Result<(), Box<dyn Error>> {
    assert_eq!(prepared.discovery.workspaces.len(), 3);
    let root_workspace = prepared
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root.is_empty())
        .ok_or("missing root Cargo workspace")?;
    assert_eq!(root_workspace.profile.compile_driver.as_str(), "cargo");
    assert!(
        root_workspace
            .record
            .packages
            .iter()
            .any(|package| { package.name == "demo" && package.manifest == "Cargo.toml" })
    );

    for &(relative_root, package_name) in expected_roots {
        let workspace = prepared
            .discovery
            .workspaces
            .iter()
            .find(|workspace| workspace.record.workspace_root == relative_root)
            .ok_or_else(|| format!("missing independent workspace {relative_root}"))?;
        assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
        assert_eq!(workspace.profile.driver_source.as_str(), "detected");
        assert!(workspace.profile.evidence.iter().any(|evidence| {
            evidence.path == format!("{relative_root}/mise.toml")
                && evidence
                    .command_or_setting
                    .contains("wrappers.cargo.command")
        }));
        assert!(workspace.record.packages.iter().any(|package| {
            package.name == package_name
                && package.manifest == format!("{relative_root}/Cargo.toml")
                && package.in_workspace
        }));
        assert!(!workspace.record.has_external_dependencies);
        let lock = fs::read_to_string(root.join(relative_root).join("Cargo.lock"))?;
        assert!(lock.contains(&format!("name = \"{package_name}\"")));
    }
    Ok(())
}

fn seed_ids_by_root<'a>(
    prepared: &'a GenerationPreparation,
    expected_roots: &[(&str, &str)],
    root_env: &str,
) -> Result<BTreeMap<&'a str, &'a str>, Box<dyn Error>> {
    let seeds: Vec<_> = prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(_, job)| job.is_dependency_seed())
        .collect();
    assert_eq!(seeds.len(), expected_roots.len());
    let unique_seed_ids: BTreeSet<_> = seeds.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(unique_seed_ids.len(), seeds.len());
    let mut seed_ids_by_root = BTreeMap::new();
    for (id, seed) in &seeds {
        validate_job_id(id)?;
        let key_envs = mbx_bundle_key_envs(seed);
        assert_eq!(key_envs.len(), 1);
        let key_env = key_envs
            .first()
            .copied()
            .ok_or("seed bundle key is missing")?;
        let workspace_root = key_env
            .get(root_env)
            .ok_or("seed bundle key omitted its workspace root")?;
        assert_seed_bundle_key_unit(seed)?;
        assert!(seed.display_name.contains(workspace_root));
        assert!(
            seed_ids_by_root
                .insert(workspace_root.as_str(), id.as_str())
                .is_none()
        );
    }
    assert_eq!(
        seed_ids_by_root.keys().copied().collect::<BTreeSet<_>>(),
        expected_roots
            .iter()
            .map(|(relative_root, _)| *relative_root)
            .collect()
    );
    assert_eq!(
        seed_ids_by_root
            .values()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        expected_roots.len()
    );
    Ok(seed_ids_by_root)
}

fn assert_rendered_seed_routing(
    prepared: &GenerationPreparation,
    expected_roots: &[(&str, &str)],
    seed_ids_by_root: &BTreeMap<&str, &str>,
    root_env: &str,
) -> Result<(), Box<dyn Error>> {
    let required = prepared
        .workflow
        .ir
        .jobs
        .get(REQUIRED_JOB_ID)
        .ok_or("missing Required job")?;
    let rendered = render_staged_tree(prepared)?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    for &(relative_root, _) in expected_roots {
        let seed_id = *seed_ids_by_root
            .get(relative_root)
            .ok_or_else(|| format!("missing seed ID for {relative_root}"))?;
        let seed = prepared
            .workflow
            .ir
            .jobs
            .get(seed_id)
            .ok_or_else(|| format!("missing seed for {relative_root}"))?;
        let seed_key_env = mbx_bundle_key_envs(seed)
            .first()
            .copied()
            .ok_or_else(|| format!("seed bundle key is missing for {relative_root}"))?;
        assert_eq!(
            seed_key_env.get(root_env).map(String::as_str),
            Some(relative_root)
        );

        let consumers: Vec<_> = prepared
            .workflow
            .ir
            .jobs
            .iter()
            .filter(|(_, job)| {
                !job.is_dependency_seed()
                    && mbx_bundle_key_envs(job)
                        .iter()
                        .any(|env| env.get(root_env).map(String::as_str) == Some(relative_root))
            })
            .collect();
        // Reader/writer branches run on mutually exclusive events, so a seed
        // root is consumed by one logical owner (a branch pair), not one job.
        assert!(
            !consumers.is_empty(),
            "missing MBX consumer for {relative_root}"
        );
        let all: BTreeSet<&str> = prepared
            .workflow
            .ir
            .jobs
            .keys()
            .map(String::as_str)
            .collect();
        let logical: BTreeSet<&str> = consumers
            .iter()
            .map(|(id, _)| logical_owner_id(&all, id.as_str()))
            .collect();
        assert_eq!(logical.len(), 1);
        for (consumer_id, consumer) in consumers {
            let consumer_keys = mbx_bundle_key_envs(consumer);
            assert_eq!(consumer_keys.len(), 1);
            let consumer_key_env = consumer_keys
                .first()
                .copied()
                .ok_or_else(|| format!("consumer bundle key is missing for {relative_root}"))?;
            assert_seed_bundle_key_unit(consumer)?;
            assert_eq!(consumer_key_env, seed_key_env);
            assert!(
                consumer
                    .steps
                    .iter()
                    .any(|step| step.role == Some(StepRole::MbxCache))
            );
            assert!(!consumer.needs.iter().any(|need| need.as_str() == seed_id));
            assert!(!required.needs.iter().any(|need| need.as_str() == seed_id));
            assert!(workflow.contains(seed_id), "{consumer_id}: {workflow}");
            assert!(workflow.contains(relative_root), "{workflow}");
        }
    }
    Ok(())
}

fn mbx_bundle_key_envs(job: &Job) -> Vec<&BTreeMap<String, String>> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::MbxBundleKey))
        .filter_map(|step| match &step.kind {
            StepKind::Shell { env, .. } => Some(env),
            _ => None,
        })
        .collect()
}

fn assert_seed_bundle_key_unit(job: &Job) -> Result<(), Box<dyn Error>> {
    let key_steps: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::MbxBundleKey))
        .collect();
    assert_eq!(key_steps.len(), 1);
    let key_step = key_steps
        .first()
        .copied()
        .ok_or("seed bundle key is missing")?;
    let StepKind::Shell { run, .. } = &key_step.kind else {
        return Err("seed bundle key is not a shell step".into());
    };
    assert_eq!(run.len(), 3);
    assert_eq!(run[0], "sh");
    assert_eq!(run[1], "-c");
    let script = &run[2];
    let compatibility_writes: Vec<_> = script
        .split("; ")
        .filter(|statement| {
            statement.starts_with("printf ") && statement.ends_with(" > \"$d/compat\"")
        })
        .collect();
    assert_eq!(compatibility_writes.len(), 1);
    let compatibility_write = compatibility_writes
        .first()
        .copied()
        .ok_or("seed compatibility writer is missing")?;
    assert_eq!(
        compatibility_write
            .split_whitespace()
            .filter(|argument| *argument == "'unit=seed'")
            .count(),
        1
    );
    Ok(())
}
