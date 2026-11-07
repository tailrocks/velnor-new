// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use serde_json::{Value, json};
use std::{error::Error, fs, path::PathBuf};
use velnor_actions_contract::{
    Plan, TASK_EXECUTION_MANIFEST_PATH, TaskExecutionManifestV1, plan_json_bytes,
};

#[path = "lane_execution_both/cli.rs"]
mod cli;

#[path = "lane_execution_both/support.rs"]
mod support;
use support::{assert_pass, assert_reject, fixture, link_dir, read_json, sha, wrapper_command};

const RUN_KEY: &str = "r190003-a1";
const BIN: &str = env!("CARGO_BIN_EXE_velnor-actions");

struct RunContext<'a> {
    temp: &'a std::path::Path,
    root: &'a std::path::Path,
    tools: &'a std::path::Path,
    head: &'a str,
    plan: &'a Plan,
    manifest: &'a TaskExecutionManifestV1,
    manifest_path: &'a std::path::Path,
    manifest_text: &'a str,
    script: &'a str,
}

#[derive(Default)]
struct RunResults {
    reports: Vec<Value>,
    matrices: Vec<Value>,
    ledgers: Vec<Value>,
    first_digest: Option<String>,
}

fn qualified_tools() -> Result<(Option<tempfile::TempDir>, PathBuf), Box<dyn Error>> {
    let (staging, tools) = support::qualified_tool_root()?;
    for path in [
        "bin/mise",
        "bin/mbx",
        "bin/cargo-nextest",
        "bin/jactionlint",
        "bin/shellcheck",
        "cargo",
        "rustup",
        "mise-data",
    ] {
        if !tools.join(path).exists() {
            return Err(format!("qualified tool root missing {path}").into());
        }
    }
    for (path, expected) in support::qualified_binary_digests()? {
        assert_eq!(
            sha(&fs::read(tools.join(path))?),
            expected,
            "unqualified {path}"
        );
    }
    support::verify_rust_toolchain(&tools)?;
    Ok((staging, tools))
}

fn execute_job(
    ctx: &RunContext<'_>,
    job_id: &str,
    results: &mut RunResults,
) -> Result<(), Box<dyn Error>> {
    let RunContext {
        temp,
        tools,
        head,
        plan,
        manifest,
        ..
    } = ctx;
    let runner = temp.join(job_id);
    fs::create_dir_all(runner.join("velnor/bin"))?;
    link_dir(&tools.join("cargo"), &runner.join("velnor/cargo"))?;
    link_dir(&tools.join("rustup"), &runner.join("velnor/rustup"))?;
    let run_dir = runner.join("velnor").join(RUN_KEY);
    fs::create_dir_all(&run_dir)?;
    fs::write(run_dir.join("plan.json"), plan_json_bytes(plan)?)?;
    let entries: Vec<_> = plan
        .matrix
        .include
        .iter()
        .filter(|entry| entry.job_id == job_id)
        .collect();
    if entries.is_empty() {
        return Err("planned job has no entries".into());
    }
    let helper = runner.join("velnor/bin").join(format!(
        "velnor-actions-{}",
        entries
            .iter()
            .find_map(|entry| manifest
                .tasks
                .values()
                .find(|record| record.matrix_id == entry.id)
                .map(|record| record.report_helper_version.as_str()))
            .ok_or("record missing")?
    ));
    fs::copy(BIN, &helper)?;
    execute_entries(ctx, job_id, &runner, &run_dir, entries, results)?;
    results
        .ledgers
        .push(cli::emit_ledger(tools, ctx.root, &runner, head, job_id)?);
    Ok(())
}

fn execute_entries(
    ctx: &RunContext<'_>,
    job_id: &str,
    runner: &std::path::Path,
    run_dir: &std::path::Path,
    entries: Vec<&velnor_actions_contract::workflow::matrix_entry::MatrixEntry>,
    results: &mut RunResults,
) -> Result<(), Box<dyn Error>> {
    let RunContext {
        root,
        tools,
        manifest,
        script,
        ..
    } = ctx;
    let RunResults {
        reports,
        matrices,
        first_digest,
        ..
    } = results;
    for entry in entries {
        let record = manifest
            .tasks
            .values()
            .find(|record| {
                record.matrix_id == entry.id
                    && record.matrix_key == entry.matrix_key
                    && record.task_id == entry.task_id
            })
            .ok_or("matching record missing")?;
        if first_digest.is_none() {
            *first_digest = Some(record.execution_digest.clone());
        }
        let out = wrapper_command(
            script,
            root,
            runner,
            tools,
            job_id,
            &manifest.generator_version,
            &record.execution_digest,
        )?
        .output()?;
        assert!(
            out.status.success(),
            "wrapper failed for {}: stdout={} stderr={}",
            entry.task_id,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let matrix_dir = run_dir.join(&entry.matrix_key);
        matrices.push(read_json(&matrix_dir.join("matrix-report.json"))?);
        for file in fs::read_dir(matrix_dir.join("tasks"))? {
            let path = file?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                reports.push(read_json(&path)?);
            }
        }
    }
    Ok(())
}

fn check_invalid_frames(
    ctx: &RunContext<'_>,
    jobs: [&str; 2],
    first_digest: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let RunContext {
        temp,
        root,
        tools,
        manifest,
        manifest_path,
        manifest_text,
        script,
        ..
    } = ctx;
    let wrong_job = jobs[1];
    let output = wrapper_command(
        script,
        root,
        &temp.join(wrong_job),
        tools,
        wrong_job,
        &manifest.generator_version,
        first_digest.ok_or("digest missing")?,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "hosted digest accepted under other job"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid declared-task execution frame")
    );
    let original_digest = first_digest.ok_or("digest missing")?;
    let mut altered = (*manifest).clone();
    let (first_key, original) = altered
        .tasks
        .iter()
        .find(|(_, record)| record.execution_digest == original_digest)
        .map(|(key, record)| (key.clone(), record.clone()))
        .ok_or("original record missing")?;
    let (other_key, other) = altered
        .tasks
        .iter()
        .find(|(_, record)| {
            record.task_id == original.task_id && record.matrix_id != original.matrix_id
        })
        .map(|(key, record)| (key.clone(), record.clone()))
        .ok_or("other lane record missing")?;
    let mut wrong_matrix = original;
    wrong_matrix.matrix_id = other.matrix_id;
    wrong_matrix.matrix_key = other.matrix_key;
    wrong_matrix.refresh_execution_digest()?;
    altered.tasks.remove(&first_key);
    altered.tasks.remove(&other_key);
    altered
        .tasks
        .insert(wrong_matrix.matrix_id.clone(), wrong_matrix.clone());
    fs::write(manifest_path, altered.marked_json()?)?;
    let output = wrapper_command(
        script,
        root,
        &temp.join(jobs[0]),
        tools,
        jobs[0],
        &manifest.generator_version,
        &wrong_matrix.execution_digest,
    )?
    .output()?;
    assert!(
        !output.status.success(),
        "wrong matrix accepted under original job"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid declared-task execution frame")
    );
    fs::write(manifest_path, manifest_text)?;
    Ok(())
}

#[test]
fn executed_rust_both_binds_wrapper_reports_and_ledgers() -> Result<(), Box<dyn Error>> {
    let (_tool_staging, tools) = qualified_tools()?;
    let (temp, root, head) = fixture()?;
    let generated = cli::generate(&root, &tools)?;
    let inventory = &generated.inventory;
    for id in ["rust-fixture__hosted", "rust-fixture__local"] {
        assert!(generated.required.iter().any(|required| required == id));
    }
    let request = json!({"schema": 1, "op": "plan-v1", "run_key": RUN_KEY, "event": "push", "head": head, "root": root, "lane_inventory": inventory});
    let response = cli::invoke(&tools, "plan-v1", &request, true)?;
    let plan: Plan = serde_json::from_value(response["plan"].clone())?;
    assert_eq!(plan.generator.sha256, sha(&fs::read(BIN)?));
    let manifest_text = &generated.manifest;
    let manifest_body = manifest_text
        .split_once('\n')
        .ok_or("manifest marker missing")?
        .1;
    let manifest: TaskExecutionManifestV1 = serde_json::from_str(manifest_body)?;
    let manifest_path = root.join(TASK_EXECUTION_MANIFEST_PATH);
    fs::create_dir_all(manifest_path.parent().ok_or("manifest parent missing")?)?;
    fs::write(&manifest_path, manifest_text)?;
    let script = &generated.script;
    let pair: Vec<_> = plan
        .jobs
        .iter()
        .filter(|job| {
            job.job_id.starts_with("rust-fixture__")
                && job.disposition == velnor_actions_contract::JobDisposition::Run
        })
        .collect();
    assert_eq!(pair.len(), 2, "fixture must run both concrete jobs");
    let ctx = RunContext {
        temp: temp.path(),
        root: &root,
        tools: &tools,
        head: &head,
        plan: &plan,
        manifest: &manifest,
        manifest_path: &manifest_path,
        manifest_text,
        script,
    };
    let mut results = RunResults::default();
    for job in &pair {
        execute_job(&ctx, &job.job_id, &mut results)?;
    }
    check_invalid_frames(
        &ctx,
        [&pair[0].job_id, &pair[1].job_id],
        results.first_digest.as_deref(),
    )?;
    let required_ids = &generated.required;
    check_merge(&tools, required_ids, &response, &json!(inventory), results)?;
    Ok(())
}

fn check_merge(
    tools: &std::path::Path,
    required_ids: &[String],
    response: &Value,
    inventory: &Value,
    results: RunResults,
) -> Result<(), Box<dyn Error>> {
    let RunResults {
        reports,
        matrices,
        ledgers,
        ..
    } = results;
    let required_jobs: Vec<_> = required_ids
        .iter()
        .map(|id| json!({"job_id": id, "conclusion": "success"}))
        .collect();
    let mut merge = json!({"schema": 1, "run_key": RUN_KEY, "actual_event": "push", "plan": response["plan"], "matrix": response["matrix"], "matrix_reports": matrices, "task_reports": reports, "required_job_ids": required_ids, "required_jobs": required_jobs, "group_ledgers": ledgers, "lane_inventory": inventory});
    assert_pass(&merge, tools)?;
    let all_ledgers = merge["group_ledgers"]
        .as_array()
        .ok_or("ledgers missing")?
        .clone();
    merge["group_ledgers"] = json!([all_ledgers[0]]);
    assert_reject(&merge, tools)?;
    let mut replay = all_ledgers[0].clone();
    replay["job"]["job_key"] = all_ledgers[1]["job"]["job_key"].clone();
    merge["group_ledgers"] = json!([all_ledgers[0], replay]);
    assert_reject(&merge, tools)?;
    Ok(())
}
