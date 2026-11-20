// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_gen::expected_with_notes;
use velnor_model::{CacheKey, CacheRoot, Host, JobId, RepoPath};
use velnor_render::render_key;
use velnor_rust::{LOCK_PATH, pin_lock};

use crate::archive::jobs_of;
use crate::base_save::{jobs, names, step};
use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::{view, workflow};

const GUARD: &str =
    "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)";
const EVENTS: &str = "github.event_name == ''push'' || github.event_name == ''schedule'' || github.event_name == ''workflow_dispatch''";
const ADMISSION: &str = "(github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed''))";

fn lane(config: &str, id: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs("0.1.81", config)?
        .into_iter()
        .find(|(job, _)| job == id)
        .map(|(_, block)| block)
        .ok_or_else(|| format!("no job {id}").into())
}

#[test]
fn a_lane_keeps_read_only_contents_and_publishes_its_trim_prefix_and_save_decision() -> TestResult {
    let block = lane("", "rust")?;
    assert!(
        block.contains("    permissions:\n      actions: read\n      contents: read\n    cache-mode: write\n    outputs:"),
        "{block}"
    );
    assert!(!block.contains("actions: write"), "{block}");
    assert!(
        block.contains("      base_save: '${{ steps.base.outputs.save }}'\n"),
        "{block}"
    );
    assert!(block.contains("      base_prefix: 'velnor-tgt-"), "{block}");
    assert!(!block.contains("cache trim"), "{block}");
    Ok(())
}

#[test]
fn one_trim_job_alone_holds_actions_write_and_runs_only_after_a_default_branch_save() -> TestResult
{
    let all = jobs("0.1.81", "shape = \"lint-test\"\n")?;
    let (_, trim) = all
        .iter()
        .find(|(id, _)| id == "rust-trim")
        .ok_or("no trim job")?;
    assert!(
        trim.contains("    needs:\n      - rust-lint\n      - rust-test\n"),
        "{trim}"
    );
    let condition =
        format!("    if: '${{{{ {ADMISSION} && (!cancelled() && {GUARD} && ({EVENTS})) }}}}'\n");
    assert!(trim.contains(&condition), "{trim}");
    assert!(
        trim.contains(
            "    permissions:\n      pull-requests: read\n      actions: write\n    cache-mode: read\n    runs-on:"
        ),
        "{trim}"
    );
    assert!(!trim.contains("checkout"), "{trim}");
    assert!(!trim.contains("contents:"), "{trim}");
    assert!(
        trim.contains("uses: tailrocks/velnor-new/actions/setup-velnor@"),
        "{trim}"
    );
    for lane in ["rust-lint", "rust-test"] {
        let delete =
            step(trim, &format!("Delete the older {lane} entries")).ok_or("no trim step")?;
        assert!(
            delete.contains(&format!("needs.{lane}.outputs.base_save == ''true''")),
            "{delete}"
        );
        assert!(
            delete.contains(&format!(
                "CACHE_PREFIX: '${{{{ needs.{lane}.outputs.base_prefix }}}}'"
            )),
            "{delete}"
        );
        assert!(
            delete.contains("GITHUB_TOKEN: '${{ github.token }}'"),
            "{delete}"
        );
        assert!(
            delete.contains("run: 'velnor-ci cache trim --prefix \"$CACHE_PREFIX\"'"),
            "{delete}"
        );
    }
    for (id, block) in &all {
        if id != "rust-trim" {
            assert!(!block.contains("actions: write"), "{id}");
        }
    }
    Ok(())
}

fn job_if(block: &str) -> String {
    block
        .lines()
        .find(|line| line.starts_with("    if: "))
        .unwrap_or("")
        .to_owned()
}

fn needed(block: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut in_needs = false;
    for line in block.lines() {
        if line == "    needs:" {
            in_needs = true;
        } else if in_needs {
            if let Some(id) = line.trim().strip_prefix("- ") {
                ids.push(id.to_owned());
            } else {
                break;
            }
        }
    }
    ids
}

fn the_budget_runs_without_a_save_and_each_trim_keeps_its_lane_guard(block: &str, lanes: &[&str]) {
    let condition = job_if(block);
    assert!(!condition.contains("needs."), "{condition}");
    for id in lanes {
        for (name, guard) in [
            (
                format!("Delete the older {id} entries"),
                format!("needs.{id}.outputs.base_save == ''true''"),
            ),
            (
                format!("Delete the older {id} sources entries"),
                format!(
                    "needs.{id}.outputs.sources_prefix != '''' && (needs.{id}.outputs.sources_saved == ''true'' || needs.{id}.outputs.sources_key != '''')"
                ),
            ),
        ] {
            let command = step(block, &name).unwrap_or("");
            assert!(command.contains(&guard), "{name}: {block}");
        }
    }
    let budget = step(block, "Enforce the repository cache budget").unwrap_or("");
    assert!(budget.contains("velnor-ci cache budget"), "{block}");
    assert!(!budget.contains("    if:"), "{budget}");
    assert!(!budget.contains("base_save"), "{budget}");
}

#[test]
fn a_single_lane_budget_runs_without_a_save_and_guards_its_trims() -> TestResult {
    let block = lane("", "rust-trim")?;
    let condition = job_if(&block);
    assert!(condition.contains("!cancelled()"), "{condition}");
    assert!(condition.contains(GUARD), "{condition}");
    assert!(condition.contains(EVENTS), "{condition}");
    assert_eq!(needed(&block), ["rust".to_owned()]);
    the_budget_runs_without_a_save_and_each_trim_keeps_its_lane_guard(&block, &["rust"]);
    Ok(())
}

#[test]
fn a_two_root_budget_waits_for_both_lanes_and_guards_their_trims() -> TestResult {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(b""))?;
    repo.files.insert(
        LOCK_PATH,
        pin_lock("0.1.81", COMMIT_FILE, SUMS, None)?.into_bytes(),
    );
    let text = workflow(&repo)?;
    let trim = jobs_of(&text)
        .into_iter()
        .find(|(id, _)| id == "rust-trim")
        .map(|(_, block)| block)
        .ok_or("no trim job")?;
    assert_eq!(
        needed(&trim),
        [
            "rust-root".to_owned(),
            "rust-lab".to_owned(),
            "rust-inputs-audit".to_owned()
        ]
    );
    the_budget_runs_without_a_save_and_each_trim_keeps_its_lane_guard(
        &trim,
        &["rust-root", "rust-lab"],
    );
    Ok(())
}

fn position(order: &[String], name: &str) -> Result<usize, Box<dyn std::error::Error>> {
    order
        .iter()
        .position(|candidate| candidate == name)
        .ok_or_else(|| format!("no step {name}").into())
}

mod publication;

