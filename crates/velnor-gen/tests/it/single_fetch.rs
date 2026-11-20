// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{
    CacheKey, CacheRoot, Concurrency, Host, Job, JobId, Permissions, PrRun, PullRequestType,
    RepoPath, RepoView, RootSlug, RunnerLabel, Step, StepKind, Triggers, WORKFLOW_PATH, When,
    WorkflowPlan,
};
use velnor_render::render;

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const FETCH_STEP: &str = "Fetch the sources of every root";
const SAVE_STEP: &str = "Save the sources cache";

fn repo(manifests: &[&str], helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(manifests, None)?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
    repo.files
        .insert(lock, text.replace("0.1.81", helper).into_bytes());
    Ok(repo)
}

fn many(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("services/payments-gateway-region-{index:02}/Cargo.toml")
            }
        })
        .collect()
}

fn savers(text: &str) -> Vec<(String, String)> {
    jobs_of(text)
        .into_iter()
        .filter(|(_, block)| step(block, SAVE_STEP).is_some())
        .collect()
}

#[test]
fn a_helper_with_sources_fetch_gets_one_step_listing_every_root_in_order() -> TestResult {
    let text = workflow(&repo(
        &["Cargo.toml", "xtask/Cargo.toml", "tools/Cargo.toml"],
        "0.1.88",
    )?)?;
    let savers = savers(&text);
    assert!(savers.len() >= 3, "{}", savers.len());
    for (id, block) in savers {
        let all = names(&block);
        let fetches: Vec<&String> = all
            .iter()
            .filter(|name| name.starts_with("Fetch the sources of "))
            .collect();
        assert_eq!(fetches, [FETCH_STEP], "{id}");
        let fetch = step(&block, FETCH_STEP).ok_or("no fetch")?;
        assert!(
            fetch
                .lines()
                .any(|line| line.trim() == "run: velnor-ci sources fetch --roots . ./tools ./xtask"),
            "{id}: {fetch}"
        );
        assert!(!fetch.contains("working-directory"), "{fetch}");
        assert!(fetch.contains("continue-on-error: true"), "{fetch}");
        assert!(
            fetch.contains("steps.base.outputs.sources == ''true''"),
            "{fetch}"
        );
        assert!(fetch.contains("cache-hit != ''true''"), "{fetch}");
        let position = |wanted: &str| all.iter().position(|name| name == wanted);
        assert!(position(FETCH_STEP) < position(SAVE_STEP), "{id}");
    }
    Ok(())
}

#[test]
fn the_sources_save_still_requires_the_one_fetch_to_succeed() -> TestResult {
    let text = workflow(&repo(&["Cargo.toml", "xtask/Cargo.toml"], "0.1.88")?)?;
    let savers = savers(&text);
    assert!(savers.len() >= 2, "{}", savers.len());
    for (id, block) in savers {
        let save = step(&block, SAVE_STEP).ok_or("no save")?;
        assert!(
            save.contains("steps.sources_fetch_all.outcome == ''success''"),
            "{id}: {save}"
        );
        let fetch = step(&block, FETCH_STEP).ok_or("no fetch")?;
        assert!(fetch.contains("id: sources_fetch_all"), "{id}: {fetch}");
    }
    Ok(())
}

#[test]
fn a_helper_without_sources_fetch_keeps_one_step_per_root_and_the_sixteen_root_refusal()
-> TestResult {
    let text = workflow(&repo(&["Cargo.toml", "xtask/Cargo.toml"], "0.1.81")?)?;
    for (id, block) in savers(&text) {
        assert!(step(&block, FETCH_STEP).is_none(), "{id}");
        for position in 0..2 {
            let fetch = step(&block, &format!("Fetch the sources of root {position}"))
                .ok_or("no per-root fetch")?;
            assert!(
                fetch.contains("velnor-ci run -- cargo fetch --locked"),
                "{id}: {fetch}"
            );
        }
    }
    let manifests = many(17);
    let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
    match expected_with_notes(&repo(&names, "0.1.81")?) {
        Err(GenerateError::TooManySourceRoots(17)) => Ok(()),
        other => Err(format!("17 roots with 0.1.81: {:?}", other.map(|_| ())).into()),
    }
}

#[test]
fn the_released_helpers_without_sources_fetch_keep_the_per_root_steps_and_the_limit() -> TestResult
{
    for helper in ["0.1.82", "0.1.83", "0.1.84", "0.1.85", "0.1.86", "0.1.87"] {
        let text = workflow(&repo(&["Cargo.toml", "xtask/Cargo.toml"], helper)?)?;
        for (id, block) in savers(&text) {
            assert!(step(&block, FETCH_STEP).is_none(), "{helper} {id}");
            assert!(
                step(&block, "Fetch the sources of root 1").is_some(),
                "{helper} {id}"
            );
        }
        let manifests = many(17);
        let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
        match expected_with_notes(&repo(&names, helper)?) {
            Err(GenerateError::TooManySourceRoots(17)) => {}
            other => return Err(format!("{helper}: {:?}", other.map(|_| ())).into()),
        }
    }
    Ok(())
}

#[test]
fn nineteen_roots_fit_the_workflow_limit_with_the_single_fetch_step() -> TestResult {
    let manifests = many(19);
    let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let text = workflow(&repo(&names, "0.1.88")?)?;
    assert!(text.len() <= 500_000, "{} bytes", text.len());
    let savers = savers(&text);
    assert!(savers.len() >= 19, "{}", savers.len());
    let (_, block) = &savers[0];
    let fetch = step(block, FETCH_STEP).ok_or("no fetch")?;
    let run = fetch
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or("no run")?;
    assert_eq!(
        run.split_whitespace().count(),
        4 + 19,
        "one word per root: {run}"
    );
    Ok(())
}

fn long_roots(count: usize) -> Result<Vec<RepoPath>, Box<dyn std::error::Error>> {
    (0..count)
        .map(|index| {
            Ok(RepoPath::new(format!(
                "r{index:02}/{}/Cargo.toml",
                "x".repeat(96)
            ))?)
        })
        .collect()
}

fn step_of(kind: StepKind) -> Step {
    Step {
        kind,
        directory: None,
        manifest: None,
        target_dir: None,
        when: When::Success,
        pr: PrRun::Always,
        gate: None,
        reads: None,
        skip_after_merge_group: None,
        skip_when_cold: false,
    }
}

fn render_fetch(manifests: Vec<RepoPath>) -> Result<String, Box<dyn std::error::Error>> {
    let lane = RootSlug::new("rust")?;
    let root = CacheRoot::new(&RepoPath::new("Cargo.toml")?, RepoPath::new("target")?);
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
        hosted_runners: velnor_model::HostedRunners::new(vec![RunnerLabel::new("ubuntu-latest")?]),
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
            records_build: false,
            selection_name: None,
            gate: None,
            skip_after_merge_group: None,
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            steps: vec![
                step_of(StepKind::SourcesFetchAll { lane, manifests }),
                step_of(StepKind::CacheSave(CacheKey::Sources {
                    host: Host,
                    lane: root,
                })),
            ],
        }],
    };
    let files = render(&plan).map_err(|found| format!("{found:?}"))?;
    Ok(files.get(&WORKFLOW_PATH).ok_or("no workflow")?.clone())
}

#[test]
fn roots_that_overflow_one_run_value_are_fetched_by_chained_steps() -> TestResult {
    let text = render_fetch(long_roots(20)?)?;
    let block = jobs_of(&text).into_iter().next().ok_or("no job")?.1;
    let all = names(&block);
    let fetches: Vec<&String> = all
        .iter()
        .filter(|name| name.starts_with(FETCH_STEP))
        .collect();
    assert!(fetches.len() >= 2, "{all:?}");
    let mut roots = 0;
    for (index, name) in fetches.iter().enumerate() {
        let fetch = step(&block, name).ok_or("no fetch")?;
        let run = fetch
            .lines()
            .find_map(|line| line.trim().strip_prefix("run: "))
            .ok_or("no run")?;
        assert!(run.chars().count() <= 2_000, "{} characters", run.len());
        roots += run
            .split_whitespace()
            .filter(|word| word.starts_with("./") || *word == ".")
            .count();
        let id = |at: usize| match at {
            0 => "sources_fetch_all".to_owned(),
            at => format!("sources_fetch_all_{at}"),
        };
        assert!(fetch.contains(&format!("id: {}", id(index))), "{fetch}");
        assert!(fetch.contains("continue-on-error: true"), "{fetch}");
        if let Some(previous) = index.checked_sub(1) {
            assert!(
                fetch.contains(&format!("steps.{}.outcome == ''success''", id(previous))),
                "{fetch}"
            );
        }
    }
    assert_eq!(roots, 20, "every root is fetched exactly once");
    let save = step(&block, SAVE_STEP).ok_or("no save")?;
    assert!(
        save.contains(&format!(
            "steps.sources_fetch_all_{}.outcome == ''success''",
            fetches.len() - 1
        )),
        "{save}"
    );
    Ok(())
}

#[test]
fn roots_that_fit_one_run_value_keep_the_single_step() -> TestResult {
    let text = render_fetch(long_roots(3)?)?;
    let block = jobs_of(&text).into_iter().next().ok_or("no job")?.1;
    let all = names(&block);
    assert_eq!(
        all.iter()
            .filter(|name| name.starts_with(FETCH_STEP))
            .count(),
        1,
        "{all:?}"
    );
    Ok(())
}
