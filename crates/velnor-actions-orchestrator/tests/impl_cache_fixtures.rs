//! Structural cache fixtures for Rust computation and isolated source producers.
//! Literal source cohorts and canonical paths prove emission, not hosted reuse.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{SourceBoundOperation, SourceProducerRole, Step, StepKind};
use velnor_actions_mise::{cache_sources, cache_trust};
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, prepare, render_staged_tree,
};
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
        fs::write(
            root.join(".velnor/config.toml"),
            format!(
                "{}\n[stacks.rust]\ncompile_driver = \"mbx\"\n",
                config_with_branch()
            ),
        )?;
    }
    Ok(dir)
}

/// Prepared workspace plus its live tempdir (MBX when `mbx`).
fn prep_for(mbx: bool) -> Result<(TempDir, GenerationPreparation), Box<dyn std::error::Error>> {
    let repo = make_workspace(mbx)?;
    let prep = prepare(repo.path())?;
    Ok((repo, prep))
}

/// Rendered workflow text for the workspace (MBX when `mbx`).
fn yaml_for(mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
    let (_repo, prep) = prep_for(mbx)?;
    let tree = render_staged_tree(&prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Steps of one IR job by id.
fn job_steps<'a>(
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

/// Rust computation jobs, excluding typed source/tool producers.
fn crate_jobs(prep: &GenerationPreparation) -> Vec<String> {
    prep.workflow
        .ir
        .jobs
        .iter()
        .filter(|(id, job)| {
            id.starts_with("rust-") && job.source_producer.is_none() && job.tool_producer.is_none()
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// `with` inputs of one action step by display name.
fn action_inputs<'a>(
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

/// Source producer membership, rather than its generated job-id spelling.
fn source_jobs(prep: &GenerationPreparation) -> Vec<(&str, &velnor_actions_contract::Job)> {
    prep.workflow
        .ir
        .jobs
        .iter()
        .filter_map(|(id, job)| {
            job.source_producer
                .as_ref()
                .filter(|meta| meta.role == SourceProducerRole::Cargo)
                .map(|_| (id.as_str(), job))
        })
        .collect()
}

#[test]
fn sources_restore_matches_its_literal_producer_cohort() -> TestResult {
    for mbx in [false, true] {
        let (_repo, prep) = prep_for(mbx)?;
        let producers = source_jobs(&prep);
        assert!(!producers.is_empty(), "source producers present");
        let crates = crate_jobs(&prep);
        assert_eq!(crates, ["rust-a", "rust-b"]);
        for id in std::iter::once("plan").chain(crates.iter().map(String::as_str)) {
            let restore = action_inputs(job_steps(&prep, id)?, "Restore Cargo sources")?;
            let prefix = restore.get("restore-keys").ok_or("source prefix")?;
            let (producer_id, producer) = producers
                .iter()
                .find(|(_, job)| {
                    job.source_producer.as_ref().is_some_and(|meta| {
                        prefix == &format!("{}-snapshot-", meta.source_identity)
                    })
                })
                .ok_or("matching source producer")?;
            let meta = producer.source_producer.as_ref().ok_or("source metadata")?;
            assert!(
                meta.source_identity
                    .starts_with("velnor-v4-cargo-source-public-")
            );
            assert!(
                !meta.source_identity.contains("${{"),
                "literal source cohort"
            );
            assert_eq!(
                restore["key"],
                format!(
                    "{}-lookup-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}",
                    meta.source_identity
                )
            );
            let save = action_inputs(&producer.steps, "Save Cargo sources")?;
            assert_eq!(save["key"], meta.save_key());
            assert_eq!(save["path"], restore["path"]);
            let consumer = &prep.workflow.ir.jobs[id];
            assert_eq!(
                consumer
                    .needs
                    .iter()
                    .any(|need| need.as_str() == *producer_id),
                id != "plan"
            );
            assert!(
                !producer
                    .needs
                    .iter()
                    .any(|need| need.as_str() == *producer_id)
            );
        }
    }
    Ok(())
}

#[test]
fn per_job_mise_keys_qualified_without_job_suffix() -> TestResult {
    for mbx in [false, true] {
        let (_repo, prep) = prep_for(mbx)?;
        let jobs = finalized_jobs(&prep)?;
        let mut keys = BTreeMap::new();
        for id in ["plan", "rust-a", "rust-b"] {
            let with = action_inputs(&jobs[id].steps, "Restore Mise tools")?;
            let key = with.get("restore-keys").ok_or("tool prefix")?;
            assert!(key.starts_with("mise-v3-"), "qualified: {key}");
            assert!(!key.contains("latest"), "pinned: {key}");
            for role in ["-plan-", "rust-", "-a-", "-b-"] {
                assert!(!key.contains(role), "no job suffix: {key}");
            }
            keys.insert(id, key.clone());
        }
        assert_eq!(keys["rust-a"], keys["rust-b"], "identical tool unions");
    }
    Ok(())
}

#[test]
fn sources_paths_cover_owned_subset_only() -> TestResult {
    for mbx in [false, true] {
        let (_repo, prep) = prep_for(mbx)?;
        let expected: Vec<String> = ["registry/index", "registry/cache", "git/db"]
            .into_iter()
            .map(|suffix| format!("{SHARED_HOME}/{suffix}"))
            .collect();
        assert_eq!(cache_sources::sources_cache_paths(SHARED_HOME)?, expected);
        let mut transports = 0;
        for job in prep.workflow.ir.jobs.values() {
            for step in &job.steps {
                if ![
                    "Restore Cargo sources",
                    "Save Cargo sources",
                    "Verify Cargo source publication",
                ]
                .contains(&step.name.as_str())
                {
                    continue;
                }
                let with = action_inputs(&job.steps, &step.name)?;
                let paths: Vec<String> = with["path"].split('\n').map(str::to_owned).collect();
                assert_eq!(paths, expected, "canonical ordered roots");
                cache_sources::validate_sources_subset(&paths, SHARED_HOME)?;
                transports += 1;
            }
        }
        assert!(transports >= 6, "readers and producer transports present");
        let yaml = yaml_for(mbx)?;
        assert!(
            yaml.contains(&expected.join("\\n")),
            "canonical roots emitted"
        );
    }
    Ok(())
}

#[test]
fn only_pure_source_producers_publish_after_verification() -> TestResult {
    for mbx in [false, true] {
        let (_repo, prep) = prep_for(mbx)?;
        let producers = source_jobs(&prep);
        assert!(!producers.is_empty());
        for (id, job) in &prep.workflow.ir.jobs {
            let saves: Vec<_> = job
                .steps
                .iter()
                .filter(|step| step.name == "Save Cargo sources")
                .collect();
            let Some(meta) = &job.source_producer else {
                assert!(saves.is_empty(), "{id} computation cannot publish sources");
                continue;
            };
            if meta.role != SourceProducerRole::Cargo {
                continue;
            }
            assert_eq!(saves.len(), 1, "one publisher per cohort");
            assert_eq!(
                saves[0].condition.as_deref(),
                Some(meta.save_condition().as_str())
            );
            let verify = job
                .steps
                .iter()
                .position(|step| step.id.as_ref() == Some(&meta.verification_step))
                .ok_or("source verification")?;
            let save = job
                .steps
                .iter()
                .position(|step| step.id.as_ref() == Some(&meta.save_step))
                .ok_or("source publication")?;
            assert!(verify < save, "verified before save");
            assert!(
                matches!(&job.steps[verify].kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::RustSourceProducer)
            );
            assert!(
                !job.steps
                    .iter()
                    .any(|step| step.name.starts_with("Fetch Cargo sources"))
            );
            let publication = action_inputs(&job.steps, "Verify Cargo source publication")?;
            assert_eq!(publication["lookup-only"], "true");
            assert_eq!(publication["key"], meta.save_key());
        }
        assert_eq!(
            yaml_for(mbx)?.matches("Save Cargo sources").count(),
            producers.len()
        );
    }
    Ok(())
}

#[test]
fn both_source_and_mbx_restore_before_crate_fetch() -> TestResult {
    let (_repo, prep) = prep_for(true)?;
    for id in crate_jobs(&prep) {
        let steps = job_steps(&prep, &id)?;
        let at = |name: &str| steps.iter().position(|step| step.name.starts_with(name));
        let fetch = at("Fetch Cargo sources").ok_or("fetch")?;
        assert!(at("Restore Cargo sources").ok_or("sources restore")? < fetch);
        assert!(at("Restore MBX objects").ok_or("MBX restore")? < fetch);
    }
    Ok(())
}

#[test]
fn cargo_only_uses_readonly_source_transport_without_compiler_cache() -> TestResult {
    let (_repo, prep) = prep_for(false)?;
    for id in std::iter::once("plan".to_owned()).chain(crate_jobs(&prep)) {
        let steps = job_steps(&prep, &id)?;
        let with = action_inputs(steps, "Restore Cargo sources")?;
        assert!(with["restore-keys"].starts_with("velnor-v4-cargo-source-public-"));
        assert!(!with.contains_key("save-if"));
        assert!(!steps.iter().any(|step| step.name == "Save Cargo sources"));
    }
    let yaml = yaml_for(false)?;
    assert!(!yaml.contains("mr-boxington-action"));
    assert!(!yaml.contains("Swatinem/rust-cache"));
    assert!(
        yaml.contains("Save Cargo sources"),
        "pure producer owns publication"
    );
    Ok(())
}

#[test]
fn checkout_cargo_config_disables_optional_public_producer() -> TestResult {
    let repo = make_workspace(true)?;
    fs::create_dir(repo.path().join(".cargo"))?;
    fs::write(
        repo.path().join(".cargo/config.toml"),
        "[build]\nrustc-wrapper = \"mbx\"\n",
    )?;
    let prep = prepare(repo.path())?;
    assert!(source_jobs(&prep).is_empty());
    for id in crate_jobs(&prep) {
        let steps = job_steps(&prep, &id)?;
        assert!(
            steps
                .iter()
                .any(|step| step.name.starts_with("Fetch Cargo sources"))
        );
        assert!(
            !steps
                .iter()
                .any(|step| step.name == "Restore Cargo sources")
        );
    }
    Ok(())
}

#[test]
fn service_report_parses_live_shape_for_sequential_runs() {
    // Fixed format sample (live `gh cache list --json` shape); the numbers
    // it carries are illustrative — real totals live in performance.md.
    let body = r#"[{"key":"velnor-v1-sources-x86_64-unknown-linux-gnu-1.98.1-aa","sizeInBytes":17568922},{"key":"mise-v1-x86_64-unknown-linux-gnu-2026.9.16-bb","sizeInBytes":65857248}]"#;
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
