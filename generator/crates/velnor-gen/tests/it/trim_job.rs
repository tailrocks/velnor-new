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
    let condition = format!(
        "    if: '${{{{ {ADMISSION} && (!cancelled() && {GUARD} && ({EVENTS})) && (needs.rust-lint.outputs.base_save == ''true'' || needs.rust-test.outputs.base_save == ''true'' || (needs.rust-test.outputs.sources_saved == ''true'' || needs.rust-test.outputs.sources_key != '''')) }}}}'\n"
    );
    assert!(trim.contains(&condition), "{trim}");
    assert!(
        trim.contains("    permissions:\n      actions: write\n    runs-on:"),
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
    let text = crate::pipeline::workflow(&crate::base_save::pinned("0.1.79", "")?)?;
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

fn admitted(condition: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = condition;
    while let Some(at) = rest.find("needs.") {
        rest = &rest[at + "needs.".len()..];
        let Some((id, tail)) = rest.split_once(".outputs.") else {
            break;
        };
        let name = tail.split(' ').next().unwrap_or("");
        if tail.contains("== ''true''") && matches!(name, "base_save" | "sources_saved") {
            found.push((id.to_owned(), name.to_owned()));
        }
    }
    found
}

fn the_job_requires_each_needed_lane_to_have_saved(block: &str) {
    let needs = needed(block);
    let condition = job_if(block);
    let found = admitted(&condition);
    for id in &needs {
        assert!(
            found.contains(&(id.clone(), "base_save".to_owned())),
            "{condition} misses the target save of {id}"
        );
    }
    assert!(
        found.iter().all(|(id, _)| needs.contains(id)),
        "{condition}"
    );
}

#[test]
fn a_single_lane_trim_job_runs_only_when_that_lane_saved() -> TestResult {
    let block = lane("", "rust-trim")?;
    let condition = job_if(&block);
    assert!(condition.contains("!cancelled()"), "{condition}");
    assert!(condition.contains(GUARD), "{condition}");
    assert!(condition.contains(EVENTS), "{condition}");
    the_job_requires_each_needed_lane_to_have_saved(&block);
    Ok(())
}

#[test]
fn a_two_root_trim_job_runs_only_when_one_of_its_lanes_saved() -> TestResult {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(b""))?;
    repo.files.insert(
        LOCK_PATH,
        pin_lock("0.1.81", COMMIT_FILE, SUMS)?.into_bytes(),
    );
    let text = workflow(&repo)?;
    let trim = jobs_of(&text)
        .into_iter()
        .find(|(id, _)| id == "rust-trim")
        .map(|(_, block)| block)
        .ok_or("no trim job")?;
    assert_eq!(
        needed(&trim),
        ["rust-root".to_owned(), "rust-lab".to_owned()]
    );
    the_job_requires_each_needed_lane_to_have_saved(&trim);
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
            assert!(save < run, "{order:?}");
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
    let sources_only = condition
        .replace("needs.rust-lint.outputs.base_save == ''true''", "false")
        .replace("needs.rust-test.outputs.base_save == ''true''", "false");
    assert!(
        sources_only
            .contains("false || false || (needs.rust-test.outputs.sources_saved == ''true''"),
        "{condition}"
    );
    let prune =
        step(trim, "Delete the older rust-test sources entries").ok_or("no sources trim")?;
    assert!(prune.contains("sources_prefix"), "{prune}");
    Ok(())
}
