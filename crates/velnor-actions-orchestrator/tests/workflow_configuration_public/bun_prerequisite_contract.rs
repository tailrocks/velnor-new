// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::repository_orchestrator_repo;
use std::error::Error;
use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_orchestrator::{finalized_jobs, prepare};

#[test]
fn task_execution_contract_rejects_bun_and_preserves_required_tool_rules()
-> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::WorkflowIr;

    let repo = repository_orchestrator_repo(false)?;
    let prepared = prepare(repo.path())?;
    let selected = prepared
        .discovery
        .proposals
        .iter()
        .find(|task| {
            task.stack_id == "rust"
                && task.task_kind == "nextest"
                && task.identity.unit_path == "Cargo.toml"
                && task.identity.compile_driver == "mbx"
        })
        .ok_or("repository Orchestrator MBX Nextest task is missing")?;
    let task_id = selected.task_id.clone();
    let mut baseline = prepared.workflow.ir.clone();
    baseline.jobs.extend(finalized_jobs(&prepared)?);
    let (argv, inputs) = task_execution_mut(&mut baseline, &task_id)?;
    strip_bun_selectors(argv, inputs)?;
    refresh_task_digest(&mut baseline, &task_id)?;
    assert!(
        baseline.validate().is_ok(),
        "baseline Orchestrator workflow must validate"
    );

    let cases = [
        ("Bun input only", ContractCase::InputOnly),
        ("Bun argv only", ContractCase::ArgvOnly),
        ("paired project Bun pin", ContractCase::ProjectRef),
        ("paired concrete Bun pin", ContractCase::Concrete),
        ("duplicate Bun argv", ContractCase::DuplicateArgv),
        ("duplicate Bun inputs", ContractCase::DuplicateInputs),
        ("unknown runtime", ContractCase::UnknownRuntime),
        ("unversioned Bun", ContractCase::Unversioned),
        ("latest Bun", ContractCase::Latest),
        ("mismatched Bun pins", ContractCase::MismatchedPins),
        ("missing Rust", ContractCase::MissingRust),
        ("missing MBX input", ContractCase::MissingMbxInput),
        ("missing Nextest", ContractCase::MissingNextest),
    ];
    for (name, case) in cases {
        let mut workflow: WorkflowIr = baseline.clone();
        let (argv, inputs) = task_execution_mut(&mut workflow, &task_id)?;
        mutate_case(case, argv, inputs)?;
        refresh_task_digest(&mut workflow, &task_id)?;
        let result = workflow.validate();
        assert!(
            result.is_err(),
            "invalid task selectors must fail for {name}"
        );
        let error = result
            .expect_err("invalid tool selection must fail")
            .to_string();
        assert!(
            error.contains("step.task.toolchain") || error.contains("step.task.argv"),
            "{name} failed for an unrelated workflow rule: {error}"
        );
    }
    Ok(())
}

fn strip_bun_selectors(
    argv: &mut Vec<String>,
    inputs: &mut ToolchainInputs,
) -> Result<(), Box<dyn Error>> {
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .ok_or("task argv separator is missing")?;
    let mut filtered = argv[..separator]
        .iter()
        .filter(|arg| !arg.starts_with("bun@"))
        .cloned()
        .collect::<Vec<_>>();
    filtered.extend_from_slice(&argv[separator..]);
    *argv = filtered;
    inputs.tools.retain(|tool| !tool.starts_with("bun@"));
    inputs.tools.sort();
    Ok(())
}

fn add_bun_selectors(
    selector: &str,
    argv: &mut Vec<String>,
    inputs: &mut ToolchainInputs,
) -> Result<(), Box<dyn Error>> {
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .ok_or("task argv separator is missing")?;
    argv.insert(separator, selector.to_owned());
    inputs.tools.push(selector.to_owned());
    inputs.tools.sort();
    Ok(())
}

fn mutate_case(
    case: ContractCase,
    argv: &mut Vec<String>,
    inputs: &mut ToolchainInputs,
) -> Result<(), Box<dyn Error>> {
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .ok_or("task argv separator is missing")?;
    match case {
        ContractCase::InputOnly => {
            inputs.tools.push("bun@${VELNOR_TOOL_BUN}".to_owned());
        }
        ContractCase::ArgvOnly => {
            argv.insert(separator, "bun@${VELNOR_TOOL_BUN}".to_owned());
        }
        ContractCase::ProjectRef => {
            add_bun_selectors("bun@${VELNOR_TOOL_BUN}", argv, inputs)?;
        }
        ContractCase::Concrete => add_bun_selectors("bun@1.4.4", argv, inputs)?,
        ContractCase::DuplicateArgv => {
            add_bun_selectors("bun@${VELNOR_TOOL_BUN}", argv, inputs)?;
            let index = argv
                .iter()
                .position(|arg| arg.starts_with("bun@"))
                .ok_or("Bun task argv selector is missing")?;
            argv.insert(index, argv[index].clone());
        }
        ContractCase::DuplicateInputs => {
            add_bun_selectors("bun@${VELNOR_TOOL_BUN}", argv, inputs)?;
            inputs.tools.push("bun@${VELNOR_TOOL_BUN}".to_owned());
            inputs.tools.sort();
        }
        ContractCase::UnknownRuntime => add_bun_selectors("node@1.4.4", argv, inputs)?,
        ContractCase::Unversioned => add_bun_selectors("bun", argv, inputs)?,
        ContractCase::Latest => add_bun_selectors("bun@latest", argv, inputs)?,
        ContractCase::MismatchedPins => {
            inputs.tools.push("bun@1.4.4".to_owned());
            inputs.tools.sort();
            argv.insert(separator, "bun@1.4.3".to_owned());
        }
        ContractCase::MissingRust => {
            inputs.tools.retain(|tool| !tool.starts_with("rust@"));
            let rust = argv[..separator]
                .iter()
                .position(|arg| arg.starts_with("rust@"))
                .ok_or("Rust task argv selector is missing")?;
            argv.remove(rust);
        }
        ContractCase::MissingMbxInput => {
            inputs
                .tools
                .retain(|tool| !tool.starts_with("mr-boxington@"));
        }
        ContractCase::MissingNextest => {
            inputs
                .tools
                .retain(|tool| !tool.starts_with("aqua:nextest-rs/nextest/cargo-nextest@"));
            let nextest = argv[..separator]
                .iter()
                .position(|arg| arg.starts_with("aqua:nextest-rs/nextest/cargo-nextest@"))
                .ok_or("Nextest task argv selector is missing")?;
            argv.remove(nextest);
        }
    }
    inputs.tools.sort();
    Ok(())
}

fn refresh_task_digest(
    workflow: &mut velnor_actions_contract::WorkflowIr,
    task_id: &str,
) -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::StepKind;

    for job in workflow.jobs.values_mut() {
        for step in &mut job.steps {
            if let StepKind::TaskExecution {
                task_id: actual,
                argv,
                task_digest,
                toolchain_inputs,
                ..
            } = &mut step.kind
                && actual == task_id
            {
                let toolchain_id =
                    velnor_actions_contract::cachekey::toolchain_id(toolchain_inputs)?;
                *task_digest =
                    velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
                        actual,
                        argv,
                        &toolchain_id,
                    )?;
                return Ok(());
            }
        }
    }
    Err(format!("TaskExecution step for {task_id} is missing").into())
}

fn task_execution_mut<'a>(
    workflow: &'a mut velnor_actions_contract::WorkflowIr,
    task_id: &str,
) -> Result<(&'a mut Vec<String>, &'a mut ToolchainInputs), Box<dyn Error>> {
    use velnor_actions_contract::StepKind;

    for job in workflow.jobs.values_mut() {
        for step in &mut job.steps {
            if let StepKind::TaskExecution {
                task_id: actual,
                argv,
                toolchain_inputs,
                ..
            } = &mut step.kind
                && actual == task_id
            {
                return Ok((argv, toolchain_inputs));
            }
        }
    }
    Err(format!("TaskExecution step for {task_id} is missing").into())
}

#[derive(Clone, Copy)]
enum ContractCase {
    ProjectRef,
    Concrete,
    InputOnly,
    ArgvOnly,
    DuplicateArgv,
    DuplicateInputs,
    UnknownRuntime,
    Unversioned,
    Latest,
    MismatchedPins,
    MissingRust,
    MissingMbxInput,
    MissingNextest,
}
