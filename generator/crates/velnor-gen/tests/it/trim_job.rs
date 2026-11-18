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
        block.contains("    permissions:\n      actions: read\n      contents: read\n    outputs:"),
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
            "    permissions:\n      pull-requests: read\n      actions: write\n    runs-on:"
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

#[test]
fn a_repository_that_saves_nothing_gets_no_trim_job() -> TestResult {
    let text =
        crate::pipeline::workflow(&crate::base_save::pinned("0.1.79", "full_run = \"off\"\n")?)?;
    assert!(!text.contains("rust-trim"), "{text}");
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

#[test]
fn target_publication_follows_its_test_build_and_input_audit() -> TestResult {
    for (config, id) in [("", "rust"), ("shape = \"lint-test\"\n", "rust-test")] {
        let order = names(&lane(config, id)?);
        let build = position(&order, "Build the tests")?;
        let save = position(&order, "Save the target cache")?;
        let run = position(&order, "Test")?;
        assert!(build < save, "{id}: {order:?}");
        if id == "rust" {
            assert!(
                run < save && position(&order, "velnor-ci inputs audit")? < save,
                "{order:?}"
            );
        } else {
            assert!(run < save, "{order:?}");
        }
    }
    let order = names(&lane("", "rust")?);
    for name in ["Lint", "Documentation"] {
        assert!(position(&order, name)? < position(&order, "Save the target cache")?);
    }
    Ok(())
}

fn resolved(text: &str) -> String {
    let mut text = text
        .replace("${{ runner.os }}", "Linux")
        .replace("${{ runner.arch }}", "X64")
        .replace("${{ steps.prepare.outputs.compat }}", "c0mpat");
    while let Some(start) = text.find("${{") {
        let end = text[start..]
            .find("}}")
            .map_or(text.len(), |end| start + end + 2);
        text.replace_range(start..end, "h");
    }
    text
}

fn target(job: &str, manifest: &str) -> Result<CacheKey, Box<dyn std::error::Error>> {
    let manifest = RepoPath::new(manifest)?;
    Ok(CacheKey::Target {
        job: JobId::new(job)?,
        root: CacheRoot::new(&manifest, RepoPath::new("target")?),
        host: Host,
    })
}

#[test]
fn no_job_and_root_pair_can_put_its_key_under_another_pairs_trim_prefix() -> TestResult {
    let mine = render_key(&target("rust", "a/Cargo.toml")?);
    let prefix = resolved(mine.restore_keys.last().ok_or("no prefix")?);
    let other = render_key(&target("rust-a-Linux-X64-c0mpat", "s/Cargo.toml")?);
    let key = resolved(&other.key);
    assert!(!key.starts_with(&prefix), "{key} is under {prefix}");
    let own = resolved(&mine.key);
    assert!(own.starts_with(&prefix), "{own} is not under {prefix}");
    Ok(())
}

fn permission_scopes(lines: &[&str], header: &str) -> Vec<String> {
    let Some(start) = lines.iter().position(|line| *line == header) else {
        return Vec::new();
    };
    let indent = header.len() - header.trim_start().len();
    lines[start + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() > indent)
        .map(|line| line.trim().to_owned())
        .collect()
}

fn pull_request_writers(text: &str) -> Vec<String> {
    let (header, _) = text.split_once("\njobs:\n").unwrap_or((text, ""));
    let head: Vec<&str> = header.lines().collect();
    let admits = [
        "  pull_request:",
        "  pull_request_target:",
        "  merge_group:",
    ]
    .iter()
    .any(|trigger| head.contains(trigger));
    if !admits {
        return Vec::new();
    }
    let inherited = permission_scopes(&head, "permissions:");
    jobs_of(text)
        .into_iter()
        .filter(|(_, block)| {
            let lines: Vec<&str> = block.lines().collect();
            let own = lines.contains(&"    permissions:");
            let scopes = if own {
                permission_scopes(&lines, "    permissions:")
            } else {
                inherited.clone()
            };
            let guarded = block.lines().any(|line| {
                line.starts_with("    if: ")
                    && line.contains(GUARD)
                    && line.contains(EVENTS)
                    && !line.contains("== ''pull_request''")
                    && !line.contains("== ''pull_request_target''")
            });
            scopes.iter().any(|scope| scope == "actions: write") && !guarded
        })
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn no_job_of_any_sample_that_can_run_on_a_pull_request_or_merge_group_holds_actions_write()
-> TestResult {
    let mut checked = 0;
    for entry in fs::read_dir(samples())? {
        let sample = entry?.path();
        let (files, _) = expected_with_notes(&load(&sample)?)?;
        for (path, text) in &files {
            checked += 1;
            let writers = pull_request_writers(text);
            assert!(
                writers.is_empty(),
                "{} {path}: {writers:?} can run on a pull request with actions: write",
                sample.display()
            );
        }
    }
    assert!(checked > 0, "{checked} workflows");
    let saving = crate::pipeline::workflow(&crate::base_save::pinned("0.1.81", "")?)?;
    assert!(saving.contains("rust-trim:"), "{saving}");
    assert!(pull_request_writers(&saving).is_empty(), "{saving}");
    Ok(())
}

#[test]
fn the_pull_request_check_flags_a_lane_that_holds_actions_write_and_passes_the_guarded_trim_job()
-> TestResult {
    let text = crate::pipeline::workflow(&crate::base_save::pinned("0.1.81", "")?)?;
    let rust_permissions = format!("  rust:\n    if: '${{{{ {ADMISSION} }}}}'\n    permissions:\n");
    let broken = text.replace(
        &format!("{rust_permissions}      actions: read\n      contents: read\n"),
        &format!("{rust_permissions}      actions: write\n"),
    );
    assert_ne!(broken, text);
    assert_eq!(pull_request_writers(&broken), ["rust"]);
    let unguarded = text.replace(&format!("({EVENTS})"), "true");
    assert_eq!(pull_request_writers(&unguarded), ["rust-trim"]);
    Ok(())
}

#[test]
fn a_run_that_saved_only_sources_still_admits_the_trim_job_and_trims_them() -> TestResult {
    let all = jobs("0.1.81", "shape = \"lint-test\"\n")?;
    let find = |id: &str| {
        all.iter()
            .find(|(job, _)| job == id)
            .map(|(_, block)| block.as_str())
            .ok_or("missing job")
    };
    let test = find("rust-test")?;
    let save = step(test, "Save the sources cache").ok_or("no sources save")?;
    let id = save
        .lines()
        .find_map(|line| line.trim().strip_prefix("id: sources_save_"))
        .ok_or("the sources save has no id")?;
    assert!(
        test.contains(&format!(
            "      sources_saved: '${{{{ steps.sources_save_{id}.outcome == ''success'' }}}}'\n"
        )),
        "{test}"
    );
    assert!(!find("rust-lint")?.contains("sources_saved"));
    let trim = find("rust-trim")?;
    let condition = job_if(trim);
    assert!(!condition.contains("needs."), "{condition}");
    let prune =
        step(trim, "Delete the older rust-test sources entries").ok_or("no sources trim")?;
    assert!(prune.contains("sources_prefix"), "{prune}");
    assert!(
        prune.contains("needs.rust-test.outputs.sources_saved == ''true''"),
        "{prune}"
    );
    assert!(!prune.contains("base_save"), "{prune}");
    let budget = step(trim, "Enforce the repository cache budget").ok_or("no budget")?;
    assert!(!budget.contains("    if:"), "{budget}");
    Ok(())
}
