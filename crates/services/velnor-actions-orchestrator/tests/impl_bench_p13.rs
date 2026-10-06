//! P13 benchmark cases 1–7: cold, warm, repeat, leaf, API, lanes, dep/toolchain.
//!
//! Every case plans the same 10-crate fixture shape so obligation sets are
//! directly comparable (`digest=` on each `bench:` line). Walls are measured
//! with `timed_rss`; `metadata_ms` is a direct-`cargo metadata` lower-bound
//! proxy for the compiler-subprocess cost inside `plan`. Queue/transfer are
//! local no-ops (`queue=na transfer_b=0`); CI values need hosted runs.

use std::collections::BTreeSet;

use velnor_actions_orchestrator::prepare;
use velnor_actions_rust_core::parse_metadata_json;

use crate::impl_common::TestResult;
use crate::impl_perf_p13::perf_fixtures_p13::{
    add_path_dep, workspace_repo, workspace_repo_linked,
};
use crate::impl_perf_p13::perf_harness_p13::{
    BenchSample, RssSampler, bench_line, commit_two, metadata_baseline_ms, obligation_digest,
    obligation_task_ids, plan_at, timed, timed_rss, toolchain_metadata,
};

/// Fixture width for every benchmark case (44 obligations, under budget).
const BENCH_CRATES: usize = 10;

/// Emit one `bench:` line for a planned sample.
fn report(
    case: &str,
    setup_ms: u128,
    plan_ms: u128,
    metadata_ms: u128,
    rss_kb: u64,
    plan: &velnor_actions_contract::Plan,
    note: &str,
) {
    bench_line(&BenchSample {
        case,
        crates: BENCH_CRATES,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan,
        note,
    });
}

/// Cold plan on a fixture never planned before (empty local caches).
#[test]
fn bench_empty_caches_cold_plan() -> TestResult {
    let (repo, setup_ms) = timed(|| workspace_repo(BENCH_CRATES));
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two(root, "src/lib.rs");
        touched.and_then(|(base, head)| plan_at(root, &base, &head))
    });
    let (plan, _) = outcome?;
    assert!(!plan.obligations.is_empty(), "obligations exist");
    let metadata_ms = metadata_baseline_ms(root, "Cargo.toml")?;
    report(
        "cold",
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        &plan,
        "first-plan",
    );
    Ok(())
}

/// Warm restore: second plan on the same commits reuses warm caches.
#[test]
fn bench_warm_restore_second_plan() -> TestResult {
    let (repo, setup_ms) = timed(|| workspace_repo(BENCH_CRATES));
    let repo = repo?;
    let root = repo.path();
    let (base, head) = commit_two(root, "src/lib.rs")?;
    let (first, cold_ms, _) = timed_rss(|| plan_at(root, &base, &head));
    let (first, _) = first?;
    let (second, warm_ms, rss_kb) = timed_rss(|| plan_at(root, &base, &head));
    let (second, _) = second?;
    assert_eq!(
        obligation_digest(&first),
        obligation_digest(&second),
        "warm plan keeps the obligation set"
    );
    let metadata_ms = metadata_baseline_ms(root, "Cargo.toml")?;
    let note = format!("cold_ms={cold_ms}");
    report(
        "warm",
        setup_ms,
        warm_ms,
        metadata_ms,
        rss_kb,
        &second,
        &note,
    );
    Ok(())
}

/// Unchanged repeat: two identical fixtures plan the same obligation set.
#[test]
fn bench_unchanged_repeat_identical() -> TestResult {
    let (repo_a, setup_ms) = timed(|| workspace_repo(BENCH_CRATES));
    let repo_a = repo_a?;
    let repo_b = workspace_repo(BENCH_CRATES)?;
    let root_a = repo_a.path();
    let root_b = repo_b.path();
    let (base_a, head_a) = commit_two(root_a, "src/lib.rs")?;
    let (base_b, head_b) = commit_two(root_b, "src/lib.rs")?;
    let (plan_a, raw_a) = plan_at(root_a, &base_a, &head_a)?;
    let (outcome, plan_ms, rss_kb) = timed_rss(|| plan_at(root_b, &base_b, &head_b));
    let (plan_b, raw_b) = outcome?;
    assert_eq!(
        obligation_task_ids(&plan_a),
        obligation_task_ids(&plan_b),
        "cross-fixture obligation sets match"
    );
    assert_eq!(raw_a.len(), raw_b.len(), "same-shape responses");
    let metadata_ms = metadata_baseline_ms(root_b, "Cargo.toml")?;
    report(
        "repeat",
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        &plan_b,
        "two-fixtures",
    );
    Ok(())
}

/// Leaf edit on a linked fixture: inventory stable, selection recorded.
#[test]
fn bench_leaf_edit_minimal_set() -> TestResult {
    let (repo, setup_ms) = timed(|| workspace_repo_linked(BENCH_CRATES));
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two(root, "crates/c009/src/lib.rs");
        touched.and_then(|(base, head)| plan_at(root, &base, &head))
    });
    let (plan, _) = outcome?;
    assert!(!plan.obligations.is_empty(), "obligations exist");
    let metadata_ms = metadata_baseline_ms(root, "Cargo.toml")?;
    report(
        "leaf",
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        &plan,
        "touch=c009",
    );
    Ok(())
}

/// Public-API edit on the shared root crate vs the leaf edit baseline.
#[test]
fn bench_public_api_edit_reverse_deps() -> TestResult {
    let (repo_api, setup_ms) = timed(|| workspace_repo_linked(BENCH_CRATES));
    let repo_api = repo_api?;
    let repo_leaf = workspace_repo_linked(BENCH_CRATES)?;
    let root_api = repo_api.path();
    let root_leaf = repo_leaf.path();
    let prep = prepare(root_api)?;
    assert!(
        !prep.discovery.workspaces[0].record.edges.is_empty(),
        "linked fixture carries dep edges"
    );
    let (base, head) = commit_two(root_api, "crates/c000/src/lib.rs")?;
    let (outcome, plan_ms, rss_kb) = timed_rss(|| plan_at(root_api, &base, &head));
    let (api_plan, _) = outcome?;
    let (leaf_base, leaf_head) = commit_two(root_leaf, "crates/c009/src/lib.rs")?;
    let (leaf_plan, _) = plan_at(root_leaf, &leaf_base, &leaf_head)?;
    assert_eq!(
        obligation_digest(&api_plan),
        obligation_digest(&leaf_plan),
        "inventory independent of the touched crate"
    );
    assert!(
        api_plan.task_ids.len() >= leaf_plan.task_ids.len(),
        "API edit selects at least the leaf set"
    );
    let metadata_ms = metadata_baseline_ms(root_api, "Cargo.toml")?;
    let note = format!("touch=c000 leaf_selected={}", leaf_plan.task_ids.len());
    report(
        "api",
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        &api_plan,
        &note,
    );
    Ok(())
}

/// Plan one request and return its obligation digest plus wall.
fn lane_plan(request: &str) -> Result<(String, u128), Box<dyn std::error::Error>> {
    let (response, wall_ms) = timed(|| velnor_actions_orchestrator::plan_internal(request));
    let value: serde_json::Value = serde_json::from_str(&response?)?;
    let plan: velnor_actions_contract::Plan = serde_json::from_value(value["plan"].clone())?;
    Ok((obligation_digest(&plan), wall_ms))
}

/// Committed lane fixtures plus plan requests and the setup wall.
type LaneSetup = (Vec<tempfile::TempDir>, Vec<String>, u128);

/// Build one plan request per lane over committed fixtures.
fn lane_requests(lanes: usize) -> Result<LaneSetup, Box<dyn std::error::Error>> {
    let mut repos = Vec::with_capacity(lanes);
    let setup_start = std::time::Instant::now();
    let mut requests = Vec::with_capacity(lanes);
    for _ in 0..lanes {
        let repo = workspace_repo(BENCH_CRATES)?;
        let (base, head) = commit_two(repo.path(), "src/lib.rs")?;
        requests.push(
            serde_json::json!({
                "schema": 1,
                "run_key": "local",
                "base": base,
                "head": head,
                "event": "pull_request",
                "root": repo.path().display().to_string(),
            })
            .to_string(),
        );
        repos.push(repo);
    }
    Ok((repos, requests, setup_start.elapsed().as_millis()))
}

/// Sequential-vs-parallel plans over `lanes` fixture clones.
fn run_lane_count(lanes: usize) -> TestResult {
    let (_repos, requests, setup_ms) = lane_requests(lanes)?;
    let mut sequential_ms = 0_u128;
    let mut digests = Vec::with_capacity(lanes);
    for request in &requests {
        let (digest, wall_ms) = lane_plan(request)?;
        sequential_ms += wall_ms;
        digests.push(digest);
    }
    let parallel_start = std::time::Instant::now();
    let mut sampler = RssSampler::start();
    let lane_walls = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(lanes);
        for request in &requests {
            handles.push(scope.spawn(|| {
                let start = std::time::Instant::now();
                let response = velnor_actions_orchestrator::plan_internal(request);
                (response, start.elapsed().as_millis())
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "lane join failed"))
            .collect::<Result<Vec<_>, &str>>()
    });
    let lane_walls = lane_walls.map_err(std::io::Error::other)?;
    let parallel_ms = parallel_start.elapsed().as_millis();
    let rss_kb = sampler.stop();
    let mut lane_ms = Vec::with_capacity(lanes);
    for (index, (response, wall_ms)) in lane_walls.into_iter().enumerate() {
        lane_ms.push(wall_ms);
        let value: serde_json::Value = serde_json::from_str(&response?)?;
        let plan: velnor_actions_contract::Plan = serde_json::from_value(value["plan"].clone())?;
        assert_eq!(
            obligation_digest(&plan),
            digests[index],
            "lane {index} matches its sequential plan"
        );
    }
    let slowest = lane_ms.iter().copied().max().unwrap_or(1).max(1);
    eprintln!(
        "bench: case=lanes lanes={lanes} crates={BENCH_CRATES} setup_ms={setup_ms} \
         sequential_ms={sequential_ms} parallel_ms={parallel_ms} rss_kb={rss_kb} \
         lane_ms=[{}] contention_pct={} queue=na transfer_b=0",
        lane_ms
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(","),
        parallel_ms.saturating_sub(slowest) * 100 / slowest,
    );
    Ok(())
}

/// Concurrent plan lanes: 2 and 4 threads over separate fixture clones.
#[test]
fn bench_concurrent_lanes_2_and_4() -> TestResult {
    run_lane_count(2)?;
    run_lane_count(4)?;
    Ok(())
}

/// Real dependency change plus a two-toolchain inventory comparison.
#[test]
fn bench_real_dep_and_toolchain_change() -> TestResult {
    let (repo, setup_ms) = timed(|| workspace_repo(BENCH_CRATES));
    let repo = repo?;
    let root = repo.path();
    let before = prepare(root)?;
    let edges_before = before.discovery.workspaces[0].record.edges.len();
    add_path_dep(root, "c009", "c008")?;
    let after = prepare(root)?;
    let edges_after = after.discovery.workspaces[0].record.edges.len();
    assert_eq!(edges_before, 0, "plain fixture has no edges");
    assert_eq!(edges_after, 1, "one real dep edge added");
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two(root, "src/lib.rs");
        touched.and_then(|(base, head)| plan_at(root, &base, &head))
    });
    let (plan, _) = outcome?;
    let metadata_ms = metadata_baseline_ms(root, "Cargo.toml")?;
    report(
        "dep",
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        &plan,
        "edge=c009-c008",
    );
    toolchain_case(root)?;
    Ok(())
}

/// Same inventory parsed under two installed toolchains, or honest skip.
fn toolchain_case(root: &std::path::Path) -> TestResult {
    let listed = std::process::Command::new("rustup")
        .args(["toolchain", "list"])
        .output()?;
    let installed = String::from_utf8_lossy(&listed.stdout).into_owned();
    let have = |prefix: &str| installed.lines().any(|line| line.starts_with(prefix));
    if !(have("1.97.1-") && have("1.98.1-")) {
        eprintln!("bench: case=toolchain status=UNMEASURED note=needs-1.97.1-and-1.98.1");
        return Ok(());
    }
    let mut walls = Vec::new();
    let mut records = Vec::new();
    for toolchain in ["1.97.1", "1.98.1"] {
        let start = std::time::Instant::now();
        let json = toolchain_metadata(root, toolchain)?;
        walls.push(start.elapsed().as_millis());
        records.push(parse_metadata_json(
            &json,
            root,
            "toolchain",
            &BTreeSet::new(),
        )?);
    }
    assert_eq!(records[0], records[1], "inventory stable across toolchains");
    eprintln!(
        "bench: case=toolchain crates={BENCH_CRATES} walls_ms=[{}] packages={} queue=na transfer_b=0",
        walls
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(","),
        records[0].packages.len(),
    );
    Ok(())
}
