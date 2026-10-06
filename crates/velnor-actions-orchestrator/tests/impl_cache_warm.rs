//! Deterministic source emission, offline reader branches, and tool repair order.
//! These structural fixtures do not establish fresh hosted runner performance.

use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::StepKind;
use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{SETUP_MISE_NAME, render::WORKFLOW_PATH};

use super::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git};

/// Two-crate workspace repo; `lock` selects the lockfile body.
fn make_workspace(lock: &str, mbx: bool) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
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
    fs::write(root.join("Cargo.lock"), lock)?;
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

/// Lockfile body pinning `members`.
fn lock_for(members: &[&str]) -> String {
    let mut body = "version = 4\n".to_owned();
    for member in members {
        body.push_str("\n[[package]]\nname = \"");
        body.push_str(member);
        body.push_str("\"\nversion = \"0.1.0\"\n");
    }
    body
}

/// Rendered workflow bytes for one workspace.
fn yaml_for(lock: &str, mbx: bool) -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_workspace(lock, mbx)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?
        .to_owned())
}

/// Explicit transport keys in render order.
fn cache_keys(yaml: &str) -> Vec<String> {
    yaml.lines()
        .filter_map(|line| line.trim().strip_prefix("key: "))
        .map(|value| value.trim_matches('"').to_owned())
        .collect()
}

#[test]
fn identical_inputs_render_identical_caches() -> TestResult {
    for mbx in [false, true] {
        let lock = lock_for(&["a", "b"]);
        let first = yaml_for(&lock, mbx)?;
        let second = yaml_for(&lock, mbx)?;
        assert_eq!(first, second, "byte-identical render (mbx={mbx})");
        let keys = cache_keys(&first);
        assert!(!keys.is_empty(), "caches present (mbx={mbx})");
        assert_eq!(keys, cache_keys(&second), "identical keys (mbx={mbx})");
    }
    Ok(())
}

#[test]
fn fetch_carries_offline_skip_branch() -> TestResult {
    for mbx in [false, true] {
        let repo = make_workspace(&lock_for(&["a", "b"]), mbx)?;
        let prep = prepare(repo.path())?;
        let mut seen_probe = false;
        for (id, job) in &prep.workflow.ir.jobs {
            if !id.starts_with("rust-")
                || job.source_producer.is_some()
                || job.tool_producer.is_some()
            {
                continue;
            }
            for step in &job.steps {
                let StepKind::Shell { run, .. } = &step.kind else {
                    continue;
                };
                let script = run.join(" ");
                if step.name.starts_with("Fetch Cargo sources") {
                    for need in [
                        "--offline",
                        "sources hit, skipping fetch",
                        "sources miss (source_missing)",
                        "--locked",
                    ] {
                        assert!(script.contains(need), "{id} fetch misses {need}");
                    }
                    if step.name.contains("(selected ") {
                        assert!(run.windows(2).any(|pair| pair == ["cargo", "tree"]));
                        assert!(
                            run.windows(2)
                                .any(|pair| pair == ["-e", "normal,build,dev"])
                        );
                        assert!(script.contains("native_tree_selected_containing"));
                    } else {
                        assert!(script.contains("fetch --locked --offline"));
                        assert!(script.contains("cargo fetch --locked"));
                    }
                    seen_probe = true;
                }
            }
        }
        assert!(seen_probe, "crate fetch probe present (mbx={mbx})");
        let mut obligations = 0;
        for job in prep.workflow.ir.jobs.values() {
            for step in &job.steps {
                let StepKind::Shell { run, env } = &step.kind else {
                    continue;
                };
                if !env
                    .get("VELNOR_TASK_ID")
                    .is_some_and(|id| id.starts_with("stack/rust/"))
                {
                    continue;
                }
                obligations += 1;
                assert!(
                    run.join(" ").contains("--offline"),
                    "offline obligation: {}",
                    step.name
                );
            }
        }
        assert!(obligations >= 2, "obligations present (mbx={mbx})");
    }
    Ok(())
}

/// Step names per job id in YAML emission order.
///
/// Two-space headers open a job window; `- name:` lines are steps.
/// Display `name:` and artifact `name:` lines carry no `- ` marker,
/// so they never join the sequence. Pseudo-jobs (`push:`) collect
/// no steps and are ignored by id lookup.
fn yaml_steps(yaml: &str) -> BTreeMap<String, Vec<String>> {
    let mut jobs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in yaml.lines() {
        let header = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && rest.ends_with(':') && !rest.contains(' '));
        if let Some(id) = header {
            current = Some(id.trim_end_matches(':').to_owned());
        } else if let (Some(id), Some(name)) = (&current, line.trim().strip_prefix("- name: ")) {
            jobs.entry(id.clone()).or_default().push(name.to_owned());
        }
    }
    jobs
}

/// Cold and warm computation jobs repair components before obligations.
#[test]
fn rust_components_repair_after_restore_before_obligations() -> TestResult {
    for mbx in [false, true] {
        let repo = make_workspace(&lock_for(&["a", "b"]), mbx)?;
        let prep = prepare(repo.path())?;
        let jobs = finalized_jobs(&prep)?;
        let tree = render_staged_tree(&prep)?;
        let yaml = tree.get(WORKFLOW_PATH).ok_or("workflow")?;
        let emitted = yaml_steps(yaml);
        for id in ["plan", "rust-a", "rust-b"] {
            let job = jobs.get(id).ok_or("computation job")?;
            assert!(job.source_producer.is_none());
            let at = |name: &str| {
                job.steps
                    .iter()
                    .position(|step| step.name == name)
                    .ok_or_else(|| std::io::Error::other(format!("{id} misses {name}")))
            };
            let restore = at("Restore Mise tools")?;
            let components = at(PREPARE_RUST_COMPONENTS_STEP)?;
            assert!(restore < components, "{id} repair after tools restore");
            assert!(
                at(SETUP_MISE_NAME)? < components,
                "{id} bootstrap before repair"
            );
            assert_component_repair(&job.steps[components], id)?;
            let names = emitted.get(id).ok_or("emitted computation job")?;
            let emitted_at = |name: &str| {
                names
                    .iter()
                    .position(|got| got == name)
                    .ok_or("emitted step")
            };
            assert!(emitted_at("Restore Mise tools")? < emitted_at(PREPARE_RUST_COMPONENTS_STEP)?);
            if id != "plan" {
                assert!(components < at("Clippy")?, "repair before obligation");
                assert!(emitted_at(PREPARE_RUST_COMPONENTS_STEP)? < emitted_at("Clippy")?);
            }
        }
    }
    Ok(())
}

fn assert_component_repair(step: &velnor_actions_contract::Step, id: &str) -> TestResult {
    if id == "plan" {
        assert_eq!(
            step.condition.as_deref(),
            Some(velnor_actions_workflow_renderer::early_plan::NEEDS_CARGO_CONDITION),
            "Plan repairs whenever Cargo preparation is needed"
        );
    } else {
        assert!(step.condition.is_none(), "cold and warm computation repair");
    }
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err("component repair must use native fixed argv".into());
    };
    for token in [
        "rustup",
        "component",
        "add",
        "--toolchain",
        "clippy",
        "rustfmt",
    ] {
        assert!(run.join(" ").contains(token), "repair misses {token}");
    }
    Ok(())
}

#[test]
fn lockfile_delta_changes_literal_source_identity() -> TestResult {
    for mbx in [false, true] {
        let lock = lock_for(&["a", "b"]);
        let first = source_identities(&lock, mbx)?;
        // A semantic lock package change must alter the admitted literal cohort.
        let changed = lock.replace("version = \"0.1.0\"", "version = \"0.2.0\"");
        let second = source_identities(&changed, mbx)?;
        assert!(!first.is_empty(), "source cohort present");
        assert_ne!(first, second, "lock content rekeys source cohorts");
        for identity in first.iter().chain(&second) {
            assert!(identity.starts_with("velnor-v4-cargo-source-public-"));
            assert!(!identity.contains("hashFiles("));
            assert!(!identity.contains("${{"));
        }
        let repo = make_workspace(&lock, mbx)?;
        fs::remove_file(repo.path().join("Cargo.lock"))?;
        let prep = prepare(repo.path())?;
        assert!(
            prep.workflow
                .ir
                .jobs
                .values()
                .all(|job| job.source_producer.is_none())
        );
        let tree = render_staged_tree(&prep)?;
        let yaml = tree.get(WORKFLOW_PATH).ok_or("workflow")?;
        assert!(!yaml.contains("Restore Cargo sources"));
        assert!(!yaml.contains("Save Cargo sources"));
    }
    Ok(())
}

fn source_identities(lock: &str, mbx: bool) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let repo = make_workspace(lock, mbx)?;
    if lock.contains("version = \"0.2.0\"") {
        for member in ["a", "b"] {
            let manifest = repo.path().join(format!("members/{member}/Cargo.toml"));
            let body = fs::read_to_string(&manifest)?;
            fs::write(
                manifest,
                body.replace("version = \"0.1.0\"", "version = \"0.2.0\""),
            )?;
        }
    }
    let prep = prepare(repo.path())?;
    Ok(prep
        .workflow
        .ir
        .jobs
        .values()
        .filter_map(|job| {
            job.source_producer
                .as_ref()
                .map(|meta| meta.source_identity.clone())
        })
        .collect())
}
