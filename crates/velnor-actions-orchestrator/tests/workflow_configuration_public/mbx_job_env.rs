// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo;

type Env = BTreeMap<String, String>;

struct RenderedJob {
    id: String,
    runs_mbx: bool,
    env: Env,
    text: String,
}

struct JobBlock<'a> {
    id: &'a str,
    lines: Vec<&'a str>,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn job_blocks(text: &str) -> Vec<JobBlock<'_>> {
    let mut blocks: Vec<JobBlock<'_>> = Vec::new();
    let mut in_jobs = false;
    for line in text.lines() {
        if !in_jobs {
            in_jobs = line == "jobs:";
            continue;
        }
        if !line.is_empty() && indent_of(line) == 0 {
            break;
        }
        match line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
        {
            Some(id) if !id.starts_with(' ') && !id.contains(' ') => blocks.push(JobBlock {
                id,
                lines: Vec::new(),
            }),
            _ => {
                if let Some(block) = blocks.last_mut() {
                    block.lines.push(line);
                }
            }
        }
    }
    blocks
}

fn env_entries(lines: &[&str], start: usize, base_indent: usize) -> Env {
    let mut env = Env::new();
    for line in &lines[start..] {
        if indent_of(line) != base_indent + 2 {
            break;
        }
        if let Some((key, value)) = line.trim_start().split_once(": ") {
            env.insert(key.to_owned(), value.trim_matches('"').to_owned());
        }
    }
    env
}

fn collect_anchors(lines: &[&str], anchors: &mut BTreeMap<String, Env>) {
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(name) = trimmed.strip_prefix("env: &") {
            anchors.insert(
                name.to_owned(),
                env_entries(lines, index + 1, indent_of(line)),
            );
        }
    }
}

fn workflow_env(text: &str) -> Env {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .position(|line| *line == "env:")
        .map(|index| env_entries(&lines, index + 1, 0))
        .unwrap_or_default()
}

fn job_env(block: &JobBlock<'_>, anchors: &BTreeMap<String, Env>) -> Result<Env, Box<dyn Error>> {
    for (index, line) in block.lines.iter().enumerate() {
        if indent_of(line) != 4 {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed == "env:" || trimmed.starts_with("env: &") {
            return Ok(env_entries(&block.lines, index + 1, 4));
        }
        if let Some(name) = trimmed.strip_prefix("env: *") {
            return anchors.get(name).cloned().ok_or_else(|| {
                format!("job {} aliases unknown env anchor {name}", block.id).into()
            });
        }
    }
    Ok(Env::new())
}

fn rendered_jobs() -> Result<Vec<RenderedJob>, Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let text = tree
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    let blocks = job_blocks(text);
    let inherited = workflow_env(text);
    let mut anchors = BTreeMap::new();
    let mut jobs = Vec::new();
    for block in &blocks {
        collect_anchors(&block.lines, &mut anchors);
        let runs_mbx = block
            .lines
            .iter()
            .any(|line| line.contains("mbx cache import"));
        jobs.push(RenderedJob {
            id: block.id.to_owned(),
            runs_mbx,
            text: block.lines.join("\n"),
            env: {
                let mut env = inherited.clone();
                env.extend(job_env(block, &anchors)?);
                env
            },
        });
    }
    Ok(jobs)
}

#[test]
fn mbx_jobs_run_the_compiler_with_shared_out_dirs_and_the_build_script_setting_per_role()
-> Result<(), Box<dyn Error>> {
    let jobs = rendered_jobs()?;
    let mbx_jobs: Vec<_> = jobs.iter().filter(|job| job.runs_mbx).collect();
    if mbx_jobs.len() < 2 {
        return Err(format!(
            "expected a seed job and a crate job, got {}",
            mbx_jobs.len()
        )
        .into());
    }
    for RenderedJob { id, env, .. } in mbx_jobs {
        for (key, expected) in [
            ("MBX_SHARE_OUT_DIR", "1"),
            (
                "MBX_BUILD_SCRIPT_EXECUTION",
                if id.starts_with("state-producer-") {
                    "0"
                } else {
                    "1"
                },
            ),
            ("MBX_CC", "0"),
            ("MBX_GC_AUTO", "0"),
            ("MBX_CACHE_EXPORT_GROUP", "velnor"),
            ("SOURCE_DATE_EPOCH", "0"),
        ] {
            let actual = env.get(key).map(String::as_str);
            if actual != Some(expected) {
                return Err(
                    format!("job {id}: {key} expected {expected:?}, got {actual:?}").into(),
                );
            }
        }
    }
    Ok(())
}

#[test]
fn jobs_without_mbx_get_no_mbx_environment() -> Result<(), Box<dyn Error>> {
    let jobs = rendered_jobs()?;
    let cargo_job = jobs
        .iter()
        .find(|job| job.id == "rust-demo")
        .ok_or("cargo crate job is missing")?;
    let plan_job = jobs
        .iter()
        .find(|job| job.id == "plan")
        .ok_or("plan job is missing")?;
    for RenderedJob {
        id, runs_mbx, env, ..
    } in [cargo_job, plan_job]
    {
        if *runs_mbx {
            return Err(format!("job {id} unexpectedly runs mbx").into());
        }
        if let Some(key) = env
            .keys()
            .find(|key| key.starts_with("MBX_") || *key == "SOURCE_DATE_EPOCH")
        {
            return Err(format!("job {id} leaked {key}").into());
        }
    }
    Ok(())
}

const NON_COMPILE_ENV: [&str; 3] = ["MBX_GC_AUTO", "MBX_CACHE_EXPORT_GROUP", "MBX_SUMMARY"];

#[test]
fn every_mbx_job_env_key_outside_the_non_compile_set_binds_the_state_key()
-> Result<(), Box<dyn Error>> {
    let jobs = rendered_jobs()?;
    let cargo_job = jobs
        .iter()
        .find(|job| job.id == "rust-demo")
        .ok_or("cargo crate job is missing")?;
    let mut checked = 0;
    for job in jobs.iter().filter(|job| job.runs_mbx) {
        for (key, value) in &job.env {
            if cargo_job.env.contains_key(key) || NON_COMPILE_ENV.contains(&key.as_str()) {
                continue;
            }
            let literal = if key == "MBX_BUILD_SCRIPT_EXECUTION" {
                format!(
                    "{key}={}",
                    velnor_actions_contract::workflow::cache_family::MBX_BSE_COMPAT
                )
            } else {
                format!("{key}={value}")
            };
            let gate = format!("${{{key}-}}");
            let gated = !job.text.contains("Publish state snapshot") || job.text.contains(&gate);
            if !job.text.contains(&literal) || !gated {
                return Err(format!(
                    "job {}: env {key} reaches the compiler but not the state key",
                    job.id
                )
                .into());
            }
            checked += 1;
        }
    }
    if checked < 4 {
        return Err(format!("only {checked} compile env bindings were checked").into());
    }
    Ok(())
}
