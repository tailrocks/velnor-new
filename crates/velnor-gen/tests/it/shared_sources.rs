// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    Concurrency, Job, JobId, Permissions, PrRun, PullRequestType, RepoPath, RootSlug, RunnerLabel,
    Step, StepKind, Triggers, WORKFLOW_PATH, When, WorkflowPlan,
};
use velnor_render::render;

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const HOST: &str = "${{ runner.os }}-${{ runner.arch }}";

fn lanes(roots: &[&str]) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(jobs_of(&workflow(&view(roots, None)?)?))
}

fn value(block: &str, field: &str) -> Option<String> {
    block.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&format!("{field}: '"))
            .and_then(|rest| rest.strip_suffix('\''))
            .map(str::to_owned)
    })
}

fn sources_keys(
    lanes: &[(String, String)],
    name: &str,
    field: &str,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let keys: BTreeSet<String> = lanes
        .iter()
        .filter_map(|(_, block)| step(block, name))
        .filter_map(|block| value(block, field))
        .collect();
    if keys.is_empty() {
        return Err(format!("no {name} step with {field}").into());
    }
    Ok(keys)
}

pub(crate) fn locks_hash(manifests: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let lock = RepoPath::new("Cargo.lock")?;
    let locks = manifests
        .iter()
        .map(|manifest| RepoPath::new(*manifest).map(|path| path.sibling(&lock)))
        .collect::<Result<Vec<_>, _>>()?;
    let plan = WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: vec![PullRequestType::Opened],
            push_branch: None,
            merge_group: false,
            schedule: None,
            workflow_dispatch: false,
        },
        concurrency: Concurrency::CancelOlderPullRequestRuns,
        permissions: Permissions::default(),
        fork_runner: RunnerLabel::new("ubuntu-latest")?,
        jobs: vec![Job {
            id: JobId::new("rust")?,
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            pr: PrRun::Always,
            runs_on: RunnerLabel::new("ubuntu-latest")?,
            permissions: Permissions::default(),
            timeout_minutes: 10,
            reports_ignored: false,
            gate: None,
            plan_outputs: Vec::new(),
            steps: vec![Step {
                kind: StepKind::SourcesLocks(locks),
                directory: None,
                target_dir: None,
                when: When::Success,
                skip_on_draft: false,
                pr: PrRun::Always,
                gate: None,
                reads: None,
                skip_after_merge_group: None,
            }],
        }],
    };
    let files = render(&plan).map_err(|found| format!("{found:?}"))?;
    let text = files.get(&WORKFLOW_PATH).ok_or("no workflow")?;
    value(text, "SOURCES_LOCKS")
        .map(|hash| hash.replace("''", "'"))
        .ok_or_else(|| format!("no lock hash in {text}").into())
}

#[test]
fn every_root_lane_restores_and_saves_the_one_shared_sources_key() -> TestResult {
    let lanes = lanes(&["xtask/Cargo.toml", "Cargo.toml"])?;
    let key = format!("velnor-src-{HOST}-all.roots-${{{{ steps.sources_locks.outputs.hash }}}}");
    let prefix = format!("velnor-src-{HOST}-all.roots-");
    for (name, field, expected) in [
        ("Restore the sources cache", "key", &key),
        ("Restore the sources cache", "restore-keys", &prefix),
        ("Save the sources cache", "key", &key),
    ] {
        assert_eq!(
            sources_keys(&lanes, name, field)?,
            BTreeSet::from([expected.clone()]),
            "{name} {field}"
        );
    }
    assert_eq!(
        sources_keys(&lanes, "Hash the sources lockfiles", "SOURCES_LOCKS")?,
        BTreeSet::from(["${{ hashFiles(''Cargo.lock'', ''xtask/Cargo.lock'') }}".to_owned()])
    );
    let owners = lanes
        .iter()
        .filter(|(_, block)| block.contains("Save the sources cache"))
        .count();
    assert!(owners >= 2, "each root keeps its own saving lane: {owners}");
    Ok(())
}

#[test]
fn a_lane_restores_the_shared_sources_payload_once_however_many_roots_it_covers() -> TestResult {
    let lanes = lanes(&["Cargo.toml", "xtask/Cargo.toml", "tools/Cargo.toml"])?;
    let audit = lanes
        .iter()
        .find(|(id, _)| id == "rust-inputs-audit")
        .ok_or("no audit lane")?;
    assert_eq!(
        audit.1.matches("name: Restore the sources cache").count(),
        1,
        "{}",
        audit.1
    );
    assert_eq!(
        audit.1.matches("name: Restore the target cache").count(),
        3,
        "{}",
        audit.1
    );
    Ok(())
}

#[test]
fn the_shared_sources_key_never_collides_with_or_prefixes_a_per_root_key() -> TestResult {
    let roots = [
        "Cargo.toml",
        "all/Cargo.toml",
        "all.roots/Cargo.toml",
        "roots/Cargo.toml",
        "xtask/Cargo.toml",
    ];
    let slugs = ["root", "all", "all_roots", "roots", "xtask"];
    assert!(RootSlug::new("all.roots").is_err());
    let text = workflow(&view(&roots, None)?)?;
    let lines: BTreeSet<String> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("key: '")
                .or_else(|| line.strip_prefix("restore-keys: '"))
                .or_else(|| line.strip_prefix("CACHE_KEY: '"))
                .or_else(|| line.strip_prefix("sources_prefix: '"))
                .or_else(|| line.strip_prefix("base_prefix: '"))
                .and_then(|rest| rest.strip_suffix('\''))
                .map(str::to_owned)
        })
        .collect();
    let (shared, others): (Vec<&String>, Vec<&String>) = lines
        .iter()
        .partition(|line| line.starts_with(&format!("velnor-src-{HOST}-all.roots-")));
    assert!(!shared.is_empty() && !others.is_empty());
    for owned in &shared {
        for other in &others {
            assert!(
                !other.starts_with(owned.as_str()),
                "{owned} prefixes {other}"
            );
            assert!(
                !owned.starts_with(other.as_str()),
                "{other} prefixes {owned}"
            );
        }
        for slug in slugs {
            let per_root = format!("velnor-src-{HOST}-{slug}-");
            assert!(!owned.starts_with(&per_root), "{per_root} prefixes {owned}");
            assert!(
                !per_root.starts_with(owned.as_str()),
                "{owned} prefixes {per_root}"
            );
        }
    }
    Ok(())
}

fn fetches(block: &str) -> Vec<String> {
    names(block)
        .into_iter()
        .filter(|name| name.starts_with("Fetch the sources of "))
        .collect()
}

#[test]
fn the_saving_path_fetches_every_root_before_the_sources_save() -> TestResult {
    let lanes = lanes(&["Cargo.toml", "xtask/Cargo.toml", "tools/Cargo.toml"])?;
    let savers: Vec<_> = lanes
        .iter()
        .filter(|(_, block)| step(block, "Save the sources cache").is_some())
        .collect();
    assert!(savers.len() >= 3, "{}", savers.len());
    for (id, block) in savers {
        let all = names(block);
        let save = all
            .iter()
            .position(|name| name == "Save the sources cache")
            .ok_or("no save")?;
        let fetched: Vec<_> = all[..save]
            .iter()
            .filter(|name| name.starts_with("Fetch the sources of "))
            .cloned()
            .collect();
        let expected: Vec<String> = (0..3)
            .map(|position| format!("Fetch the sources of root {position}"))
            .collect();
        assert_eq!(fetched, expected, "{id}");
        let mut directories = BTreeSet::new();
        for (position, name) in expected.iter().enumerate() {
            let fetch = step(block, name).ok_or("no fetch")?;
            assert!(
                fetch.contains("velnor-ci run -- cargo fetch --locked"),
                "{fetch}"
            );
            assert!(fetch.contains("continue-on-error: true"), "{fetch}");
            assert!(fetch.contains("!cancelled() && "), "{fetch}");
            assert!(
                fetch.contains("steps.base.outputs.sources == ''true''"),
                "{fetch}"
            );
            assert!(fetch.contains("cache-hit != ''true''"), "{fetch}");
            directories.insert(
                fetch
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("working-directory: "))
                    .unwrap_or_default()
                    .to_owned(),
            );
            assert_eq!(
                fetch.contains(&format!(
                    "steps.sources_fetch_{}.outcome == ''success''",
                    position.wrapping_sub(1)
                )),
                position > 0,
                "{fetch}"
            );
        }
        assert_eq!(
            directories,
            BTreeSet::from([String::new(), "tools".to_owned(), "xtask".to_owned()]),
            "{id}"
        );
        let decide =
            step(block, "Decide whether to save the build directory").ok_or("no decide")?;
        assert!(decide.contains("default_branch"), "{decide}");
    }
    Ok(())
}

#[test]
fn lanes_that_do_not_save_the_sources_run_no_fetch() -> TestResult {
    let lanes = jobs_of(&workflow(&view(
        &["Cargo.toml", "xtask/Cargo.toml"],
        Some(b"shape = \"lint-test\"\n"),
    )?)?);
    for (id, block) in &lanes {
        assert_eq!(
            step(block, "Save the sources cache").is_some(),
            !fetches(block).is_empty(),
            "{id}"
        );
    }
    assert!(
        lanes
            .iter()
            .any(|(id, block)| id.starts_with("rust-lint") && fetches(block).is_empty())
    );
    let (_, audit) = lanes
        .iter()
        .find(|(id, _)| id == "rust-inputs-audit")
        .ok_or("no audit")?;
    assert_eq!(fetches(audit), Vec::<String>::new());
    Ok(())
}
