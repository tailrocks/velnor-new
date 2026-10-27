// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
};
use velnor_actions_orchestrator::GenerationPreparation;
use velnor_actions_workflow_renderer::{
    native_authority::{
        LogicalJobFact, LogicalJobSource, NativeAuthority, NativeAuthorityInput,
        NativeRustProvider, NativeRustSource, NativeTaskFact, TofuTaskFact, WorkflowSourceFact,
    },
    native_job_builders::PlanJobToolNeeds,
};

pub(crate) fn assert_tofu_inventory_rejected(
    preparation: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let task = TofuTaskFact {
        root: "infra".to_owned(),
        kind: velnor_actions_tofu::TofuTaskKind::Fmt,
        configuration: "default".to_owned(),
    };
    let tasks = [task.clone()];
    let jobs = [logical_job(&task)];
    let duplicates = [task.clone(), task];
    let sources = source_facts(preparation)?;
    let input = NativeAuthorityInput {
        config: &preparation.config,
        runner: "ubuntu-26.04",
        published_generator_lock: None,
        consumer_manifest_json: preparation.discovery.consumer_manifest_json.as_deref(),
        tool_pins: &[],
        rust_source: NativeRustSource::ConfigOrDefault,
        rust_provider: NativeRustProvider::Rustup,
        cargo_workspaces: &[],
        workflow_sources: &sources,
        generated_sources: &[],
        rust_tasks: &[],
        tofu_tasks: &tasks,
        gradle_roots: &[],
        logical_jobs: &jobs,
        workspace_format: None,
        tools: PlanJobToolNeeds {
            opentofu: true,
            ..PlanJobToolNeeds::default()
        },
        rust_detected: false,
    };
    NativeAuthority::build(input)
        .map_err(|error| io::Error::other(format!("valid ToFu inventory rejected: {error:?}")))?;
    for (declared, logical) in [
        (tasks.as_slice(), [].as_slice()),
        ([].as_slice(), jobs.as_slice()),
        (duplicates.as_slice(), jobs.as_slice()),
    ] {
        let changed = NativeAuthorityInput {
            tofu_tasks: declared,
            logical_jobs: logical,
            tools: PlanJobToolNeeds {
                opentofu: !declared.is_empty(),
                ..input.tools
            },
            ..input
        };
        let error = NativeAuthority::build(changed)
            .err()
            .ok_or_else(|| io::Error::other("inconsistent ToFu inventory accepted"))?;
        assert!(
            format!("{error:?}").contains("native_tofu_task_inventory"),
            "unexpected error: {error:?}"
        );
    }
    Ok(())
}

fn logical_job(task: &TofuTaskFact) -> LogicalJobFact {
    let ids = velnor_actions_contract::assign_crate_job_ids(
        &BTreeSet::from([("infra".to_owned(), "infra".to_owned(), "default".to_owned())]),
        velnor_actions_contract::TOFU_JOB_ID_PREFIX,
    );
    LogicalJobFact {
        id: ids[&("infra".to_owned(), "default".to_owned())].clone(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        needs: Vec::new(),
        outputs: BTreeMap::new(),
        source: LogicalJobSource::Task(NativeTaskFact {
            compile_driver: None,
            unit: "infra".to_owned(),
            display_name: "infra".to_owned(),
            configuration: "default".to_owned(),
            rust_manifest: None,
            tofu: vec![task.clone()],
        }),
    }
}

fn source_facts(preparation: &GenerationPreparation) -> Result<Vec<WorkflowSourceFact>, io::Error> {
    ["mise.toml", "mise.lock", "rust-toolchain.toml"]
        .into_iter()
        .map(|path| {
            let sha256 = match std::fs::read(preparation.root.join(path)) {
                Ok(bytes) => Some(crate::sha256(&bytes)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            Ok(WorkflowSourceFact {
                path: path.to_owned(),
                sha256,
            })
        })
        .collect()
}
