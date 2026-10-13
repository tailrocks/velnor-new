//! P13 perf harness: plan timing plus obligation extraction.
//!
//! Included via `#[path]` from `impl_perf_p13`, so the parent wires a
//! single `mod` line for the whole perf suite.

use std::fs;
use std::path::Path;
use std::time::Instant;

use velnor_actions_contract::Plan;
use velnor_actions_orchestrator::{
    PlanPhaseTimings, plan_internal, plan_internal_with_phase_timings,
};

use crate::impl_common::{git, git_line};

/// Wall time of `op` in whole milliseconds plus its value.
pub(crate) fn timed<T>(op: impl FnOnce() -> T) -> (T, u128) {
    let start = Instant::now();
    let value = op();
    (value, start.elapsed().as_millis())
}

/// Commit everything twice, touching `touch_rel` for the second commit.
/// Returns `(base, head)` shas for planning.
pub(crate) fn commit_two(
    root: &Path,
    touch_rel: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let touch = root.join(touch_rel);
    let mut body = fs::read_to_string(&touch)?;
    body.push_str("pub fn g() {}\n");
    fs::write(&touch, body)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    Ok((base, head))
}

/// Plan `head` against `base`; returns the validated plan plus raw JSON.
pub(crate) fn plan_at(
    root: &Path,
    base: &str,
    head: &str,
) -> Result<(Plan, String), Box<dyn std::error::Error>> {
    let request = plan_request(root, base, head);
    decode_plan(plan_internal(&request)?)
}

/// Plan through the same entrypoint while collecting invocation-local phases.
pub(crate) fn plan_at_with_phase_timings(
    root: &Path,
    base: &str,
    head: &str,
) -> Result<(Plan, String, PlanPhaseTimings), Box<dyn std::error::Error>> {
    let request = plan_request(root, base, head);
    let (response, phases) = plan_internal_with_phase_timings(&request)?;
    let (plan, response) = decode_plan(response)?;
    Ok((plan, response, phases))
}

fn plan_request(root: &Path, base: &str, head: &str) -> String {
    serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    })
    .to_string()
}

fn decode_plan(response: String) -> Result<(Plan, String), Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((plan, response))
}

/// Commit twice and plan head; the standard perf-case plan run.
pub(crate) fn plan_two_commits(
    root: &Path,
    touch_rel: &str,
) -> Result<(Plan, String), Box<dyn std::error::Error>> {
    let (base, head) = commit_two(root, touch_rel)?;
    plan_at(root, &base, &head)
}

/// Sorted task IDs across every obligation.
pub(crate) fn obligation_task_ids(plan: &Plan) -> Vec<String> {
    let mut ids: Vec<String> = plan.obligations.iter().map(|o| o.task_id.clone()).collect();
    ids.sort();
    ids
}

/// Machine-readable perf line on stderr (visible with `--nocapture`).
pub(crate) fn perf_line(op: &str, crates: usize, wall_ms: u128, plan: &Plan) {
    eprintln!(
        "perf: op={op} crates={crates} wall_ms={wall_ms} obligations={} matrix_entries={}",
        plan.obligations.len(),
        plan.matrix.include.len()
    );
}

/// Current process RSS in KiB via `ps` (`None` when unreadable).
pub(crate) fn rss_kb_now() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p"])
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Peak-RSS sampler: polls own RSS every 10 ms until stopped.
pub(crate) struct RssSampler {
    /// Stop flag shared with the sampler thread.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Sampler thread; joined by [`RssSampler::stop`].
    thread: Option<std::thread::JoinHandle<u64>>,
}

impl RssSampler {
    /// Start sampling in the background.
    pub(crate) fn start() -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let mut peak = 0_u64;
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(rss) = rss_kb_now() {
                    peak = peak.max(rss);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            peak
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    /// Stop and return the sampled peak RSS in KiB (0 when unusable).
    pub(crate) fn stop(&mut self) -> u64 {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.take().and_then(|t| t.join().ok()).unwrap_or(0)
    }
}

/// Wall time plus sampled peak RSS of `op` (test-process peak, not
/// isolated plan RSS: the sampler sees the whole test binary).
pub(crate) fn timed_rss<T>(op: impl FnOnce() -> T) -> (T, u128, u64) {
    let mut sampler = RssSampler::start();
    let start = Instant::now();
    let value = op();
    let wall_ms = start.elapsed().as_millis();
    let peak = sampler.stop();
    (value, wall_ms, peak)
}

/// Direct `cargo metadata --no-deps --offline` wall in ms on one manifest:
/// a lower-bound proxy for the compiler-subprocess cost inside `plan`.
pub(crate) fn metadata_baseline_ms(
    root: &Path,
    manifest: &str,
) -> Result<u128, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let output = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--manifest-path",
        ])
        .arg(root.join(manifest))
        .output()?;
    if !output.status.success() {
        return Err("direct cargo metadata failed".into());
    }
    Ok(start.elapsed().as_millis())
}

/// Raw `cargo metadata` JSON under one rustup toolchain.
pub(crate) fn toolchain_metadata(
    root: &Path,
    toolchain: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("rustup")
        .args([
            "run",
            toolchain,
            "cargo",
            "metadata",
            "--format-version",
            "1",
        ])
        .arg("--no-deps")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed under {toolchain}").into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Short digest over the sorted obligation task IDs: equal sets hash equal.
pub(crate) fn obligation_digest(plan: &Plan) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    obligation_task_ids(plan).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// One benchmark sample: walls plus the planned obligation set.
#[derive(Debug)]
pub(crate) struct BenchSample<'a> {
    /// Case name (`cold`, `warm`, `leaf`, ...).
    pub case: &'a str,
    /// Fixture width in crates.
    pub crates: usize,
    /// Fixture construction wall in ms.
    pub setup_ms: u128,
    /// `plan_internal` wall in ms.
    pub plan_ms: u128,
    /// Direct-`cargo metadata` proxy wall in ms.
    pub metadata_ms: u128,
    /// Sampled test-process peak RSS in KiB.
    pub rss_kb: u64,
    /// Planned obligation set.
    pub plan: &'a Plan,
    /// Free-form case note.
    pub note: &'a str,
}

/// Machine-readable benchmark line: queue/transfer are local no-ops
/// (recorded `na`/`0`), compiler is the direct-metadata proxy.
pub(crate) fn bench_line(sample: &BenchSample<'_>) {
    bench_line_inner(sample, None);
}

/// Emit the existing benchmark line plus named per-call phase fields.
pub(crate) fn bench_line_with_phase_timings(sample: &BenchSample<'_>, phases: PlanPhaseTimings) {
    bench_line_inner(sample, Some(phases));
}

fn bench_line_inner(sample: &BenchSample<'_>, phases: Option<PlanPhaseTimings>) {
    let phases = phases.map_or_else(String::new, |phases| {
        format!(
            " plan_internal_us={} prepare_us={} metadata_commands={} metadata_run_us={} metadata_parse_us={} generator_sha_calls={} generator_sha_us={}",
            phases.plan_internal_us,
            phases.prepare_us,
            phases.metadata_commands,
            phases.metadata_run_us,
            phases.metadata_parse_us,
            phases.generator_sha_calls,
            phases.generator_sha_us,
        )
    });
    eprintln!(
        "bench: case={} crates={} setup_ms={} plan_ms={} metadata_ms={} rss_kb={} \
         obligations={} selected={} digest={}{} queue=na transfer_b=0 note={}",
        sample.case,
        sample.crates,
        sample.setup_ms,
        sample.plan_ms,
        sample.metadata_ms,
        sample.rss_kb,
        sample.plan.obligations.len(),
        sample.plan.task_ids.len(),
        obligation_digest(sample.plan),
        phases,
        sample.note,
    );
}

#[cfg(test)]
mod phase_timing_tests {
    use super::{
        commit_two, obligation_digest, obligation_task_ids, plan_at, plan_at_with_phase_timings,
    };
    use crate::impl_common::TestResult;
    use crate::impl_perf_p13::perf_fixtures_p13::workspace_repo;

    #[test]
    fn phase_timing_preserves_plan_and_workspace_inventory_reuse() -> TestResult {
        let repo = workspace_repo(1)?;
        let root = repo.path();
        let (base, head) = commit_two(root, "src/lib.rs")?;
        let (plain, plain_response) = plan_at(root, &base, &head)?;
        let (profiled, profiled_response, phases) = plan_at_with_phase_timings(root, &base, &head)?;

        assert_eq!(
            profiled_response, plain_response,
            "response bytes are unchanged"
        );
        assert_eq!(
            profiled.plan_id, plain.plan_id,
            "plan identity is unchanged"
        );
        assert_eq!(
            obligation_task_ids(&profiled),
            obligation_task_ids(&plain),
            "obligation identities are unchanged"
        );
        assert_eq!(obligation_digest(&profiled), obligation_digest(&plain));
        assert_eq!(profiled.packages.len(), 2, "root and member are discovered");
        assert_eq!(
            phases.metadata_commands, 1,
            "the root inventory is reused for its member"
        );
        assert_eq!(phases.generator_sha_calls, 1, "identity is hashed once");
        Ok(())
    }
}
