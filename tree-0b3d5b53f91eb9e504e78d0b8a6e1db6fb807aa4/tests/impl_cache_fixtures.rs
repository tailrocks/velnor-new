//! P08 cache fixtures: per-job keys/paths plus the service-report path.
//!
//! Renders a two-crate workspace (MBX and Cargo-only variants) and asserts
//! every job's cache identity: one shared sources key and path set, one
//! qualified Mise key shape per tool union (never job-suffixed), a single
//! plan writer, and restore-before-fetch order. The service-report test
//! pins the `gh cache list --json` reporting path on a fixed format sample
//! (live numbers live in `docs/implemented/performance.md`, never here).

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_mise::{cache_sources, cache_trust};
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git};

/// Owned Cargo home expression shared by writers and readers.
const SHARED_HOME: &str = "${{ runner.temp }}/velnor/cargo";

/// Two-crate workspace repo with a root lockfile (uncommitted).
fn make_workspace(mbx: bool) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"members/a\", \"members/b\"]\n",
    )?;
    for member in ["a", "b"] {
        let dir = root.join("members").join(member);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.1.0\"\n",
    )?;
    if mbx {
        let cargo_dir = root.join(".cargo");
        fs::create_dir_all(&cargo_dir)?;
        fs::write(
            cargo_dir.join("config.toml"),
            "[build]\nrustc-wrapper = \"mbx\"\n",
        )?;
    }
    Ok(dir)
}

/// Prepared workspace plus its live tempdir (MBX when `mbx`).
pub(super) fn prep_for(
    mbx: bool,
) -> Result<(TempDir, GenerationPreparation), Box<dyn std::error::Error>> {
    let repo = make_workspace(mbx)?;
    let prep = prepare(repo.path())?;
    Ok((repo, prep))
}

/// Rendered workflow text for the workspace (MBX when `mbx`).
pub(super) fn yaml_for(mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
    let (_repo, prep) = prep_for(mbx)?;
    let tree = render_staged_tree(&prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Steps of one IR job by id.
pub(super) fn job_steps<'a>(
    prep: &'a GenerationPreparation,
    job: &str,
) -> Result<&'a Vec<Step>, Box<dyn std::error::Error>> {
    prep.workflow
        .ir
        .jobs
        .get(job)
        .map(|found| &found.steps)
        .ok_or_else(|| std::io::Error::other(format!("missing {job}")).into())
}

/// Crate-job ids in render order (every `rust-*` job).
pub(super) fn crate_jobs(prep: &GenerationPreparation) -> Vec<String> {
    prep.workflow
        .ir
        .jobs
        .keys()
        .filter(|id| id.starts_with("rust-"))
        .cloned()
        .collect()
}

/// `with` inputs of one action step by display name.
pub(super) fn action_inputs<'a>(
    steps: &'a [Step],
    name: &str,
) -> Result<&'a BTreeMap<String, String>, Box<dyn std::error::Error>> {
    for step in steps {
        if step.name == name {
            if let StepKind::Action { with, .. } = &step.kind {
                return Ok(with);
            }
            return Err(std::io::Error::other(format!("{name} is not an action")).into());
        }
    }
    Err(std::io::Error::other(format!("missing {name}")).into())
}

/// (`job`, V2 key) pairs in render order (cache steps are render-inserted).
fn tools_keys_by_job(yaml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut job = String::new();
    let mut in_jobs = false;
    let mut in_tools_restore = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if in_jobs && line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            line.trim().trim_end_matches(':').clone_into(&mut job);
            in_tools_restore = false;
        }
        if let Some(name) = line.trim().strip_prefix("- name: ") {
            in_tools_restore = name.trim_matches('"') == "Restore Mise tools";
        }
        if in_tools_restore && let Some(value) = line.trim().strip_prefix("key: ") {
            out.push((job.clone(), value.trim_matches('"').to_owned()));
            in_tools_restore = false;
        }
    }
    out
}

/// Every cache `path:` input entry (one quoted line, `\n`-separated).
fn cache_path_entries(yaml: &str) -> Vec<String> {
    yaml.lines()
        .filter(|line| line.trim().starts_with("path: "))
        .flat_map(|line| {
            line.trim()
                .strip_prefix("path: ")
                .unwrap_or("")
                .trim_matches('"')
                .split("\\n")
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn per_job_sources_keys_identical_plan_and_crates() -> TestResult {
    let (_repo, prep) = prep_for(true)?;
    let crates = crate_jobs(&prep);
    assert_eq!(crates.len(), 2, "two crate jobs: {crates:?}");
    let plan_restore = action_inputs(job_steps(&prep, "plan")?, "Restore Cargo sources")?
        .get("key")
        .cloned();
    let plan_save = action_inputs(job_steps(&prep, "plan")?, "Save Cargo sources")?
        .get("key")
        .cloned();
    assert_eq!(plan_restore, plan_save, "writer restores what it saves");
    let key = plan_restore.ok_or("plan key")?;
    assert!(
        key.starts_with("velnor-v1-sources-"),
        "shared prefix: {key}"
    );
    assert!(key.contains("hashFiles("), "lock-pinned: {key}");
    for job in &crates {
        let got = action_inputs(job_steps(&prep, job)?, "Restore Cargo sources")?
            .get("key")
            .cloned();
        assert_eq!(got.as_deref(), Some(key.as_str()), "{job} restores the key");
    }
    for banned in ["-plan-", "-demo", "rust-"] {
        assert!(!key.contains(banned), "no job id in key: {key}");
    }
    Ok(())
}

#[test]
fn per_job_mise_keys_qualified_without_job_suffix() -> TestResult {
    for mbx in [false, true] {
        let yaml = yaml_for(mbx)?;
        let pairs = tools_keys_by_job(&yaml);
        assert!(pairs.len() >= 3, "plan plus crates (mbx={mbx}): {pairs:?}");
        for (job, key) in &pairs {
            assert!(!job.is_empty(), "key inside a job: {key}");
            assert!(key.starts_with("mise-tools-v2-"), "qualified: {key}");
            assert!(!key.contains("latest"), "pinned: {key}");
            for role in ["-plan", "rust-", "-a-", "-b-"] {
                assert!(!key.contains(role), "no role suffix: {key}");
            }
        }
        let key_for = |want: &str| {
            pairs
                .iter()
                .find(|(job, _)| job == want)
                .map(|(_, key)| key.clone())
        };
        assert_eq!(
            key_for("rust-a"),
            key_for("rust-b"),
            "identical unions share (mbx={mbx}): {pairs:?}"
        );
        assert!(key_for("rust-a").is_some() && key_for("plan").is_some());
    }
    Ok(())
}

#[test]
fn sources_paths_cover_owned_subset_only() -> TestResult {
    let (_repo, prep) = prep_for(true)?;
    let expected = cache_sources::sources_cache_paths(SHARED_HOME).expect("subset");
    assert_eq!(expected.len(), 3, "registry and Git source subset");
    let mut jobs = vec!["plan".to_owned()];
    jobs.extend(crate_jobs(&prep));
    for job in &jobs {
        let steps = job_steps(&prep, job)?;
        let restore = action_inputs(steps, "Restore Cargo sources")?
            .get("path")
            .cloned()
            .ok_or("restore paths")?;
        let archived: Vec<String> = restore.split('\n').map(str::to_owned).collect();
        cache_sources::validate_sources_subset(&archived, SHARED_HOME).expect("valid");
        assert_eq!(archived, expected, "{job} archives the subset");
    }
    let yaml = yaml_for(true)?;
    let entries = cache_path_entries(&yaml);
    assert!(!entries.is_empty(), "path inputs present");
    let mut snapshot = 0;
    let mut tools = 0;
    for entry in &entries {
        for banned in ["credentials", "registry/src", "~/.cargo"] {
            assert!(
                !entry.contains(banned),
                "snapshot excludes {banned}: {entry}"
            );
        }
        if entry.starts_with(&format!("{SHARED_HOME}/")) {
            if expected.iter().any(|ok| ok == entry) {
                snapshot += 1;
            } else if [
                format!("{SHARED_HOME}/.crates.toml"),
                format!("{SHARED_HOME}/.crates2.json"),
                format!("{SHARED_HOME}/bin"),
            ]
            .contains(entry)
            {
                tools += 1;
            } else {
                panic!("owned-home path has no cache owner: {entry}");
            }
        }
    }
    assert!(snapshot >= 6, "snapshot paths rendered: {snapshot}");
    assert!(tools >= 3, "tool paths move to V2 tools archive: {tools}");
    Ok(())
}

#[test]
fn single_writer_plan_saves_crates_restore_only() -> TestResult {
    let yaml = yaml_for(true)?;
    assert_eq!(
        yaml.matches("Save Cargo sources").count(),
        1,
        "exactly one save step"
    );
    let (_repo, prep) = prep_for(true)?;
    let plan: Vec<String> = job_steps(&prep, "plan")?
        .iter()
        .map(|step| step.name.clone())
        .collect();
    assert!(cache_sources::is_trusted_writer("plan"));
    assert!(!cache_sources::is_trusted_writer("rust-a"));
    let fetch = plan.iter().position(|n| n == "Fetch Cargo sources");
    let save = plan.iter().position(|n| n == "Save Cargo sources");
    assert!(fetch < save, "save after the writer fetch: {plan:?}");
    for job in crate_jobs(&prep) {
        let names: Vec<String> = job_steps(&prep, &job)?
            .iter()
            .map(|step| step.name.clone())
            .collect();
        assert!(
            names.contains(&"Restore Cargo sources".to_owned()),
            "{job}: {names:?}"
        );
        assert!(
            !names.contains(&"Save Cargo sources".to_owned()),
            "{job} never saves: {names:?}"
        );
    }
    Ok(())
}

#[test]
fn restore_mbx_fetch_order_every_crate_job() -> TestResult {
    let (_repo, prep) = prep_for(true)?;
    for job in crate_jobs(&prep) {
        let steps = job_steps(&prep, &job)?;
        let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
        let names: Vec<String> = steps.iter().map(|step| step.name.clone()).collect();
        cache_sources::check_restore_before_fetch(&roles, true).expect("order");
        cache_sources::check_steps_before_fetch(
            &names,
            &["Restore Cargo sources", "Restore MBX objects"],
        )
        .expect("order");
        let at = |want: &str| names.iter().position(|n| n == want);
        let (Some(restore), Some(mbx), Some(fetch)) = (
            at("Restore Cargo sources"),
            at("Restore MBX objects"),
            at("Fetch Cargo sources"),
        ) else {
            return Err(format!("{job} misses cache steps: {names:?}").into());
        };
        assert!(restore < mbx && mbx < fetch, "{job}: {names:?}");
    }
    Ok(())
}

#[test]
fn service_report_parses_live_shape_for_sequential_runs() {
    // Fixed format sample (live `gh cache list --json` shape); the numbers
    // it carries are illustrative — real totals live in performance.md.
    let body = r#"[{"key":"velnor-v1-sources-x86_64-unknown-linux-gnu-1.98.1-aa","sizeInBytes":17568922},{"key":"mise-tools-v2-typed-runtime-bb","sizeInBytes":65857248}]"#;
    let report =
        cache_trust::summarize_cache_usage(body, 10_737_418_240, 17_568_922, 3).expect("report");
    assert_eq!(report.active_bytes, 17_568_922 + 65_857_248);
    assert_eq!(report.count, 2);
    assert_eq!(report.headroom_bytes, 10_737_418_240 - report.active_bytes);
    assert_eq!(report.aggregate_transfer_bytes, 17_568_922 * 3);
    eprintln!(
        "cache: stored={} transfer={} headroom={} entries={}",
        report.stored_bytes, report.aggregate_transfer_bytes, report.headroom_bytes, report.count
    );
}
