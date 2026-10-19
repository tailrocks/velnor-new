//! `[stacks.rust.crates.<package>] build_jobs` reaches every Cargo step of that crate job.

use std::collections::BTreeMap;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

const MANIFEST: &str = ".github/velnor/task-execution-manifest-v1.json";

fn config(extra: &str) -> String {
    format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra}")
}

/// `CARGO_BUILD_JOBS` of every task in the rendered execution manifest.
fn build_jobs_by_task(
    config: &str,
) -> Result<BTreeMap<String, Option<String>>, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let manifest = tree.get(MANIFEST).ok_or("manifest not rendered")?;
    let (_marker, json) = manifest.split_once('\n').ok_or("manifest marker")?;
    let parsed: serde_json::Value = serde_json::from_str(json)?;
    let tasks = parsed["tasks"].as_object().ok_or("manifest tasks")?;
    Ok(tasks
        .iter()
        .map(|(id, task)| {
            let jobs = task["env"]["CARGO_BUILD_JOBS"].as_str().map(str::to_owned);
            (id.clone(), jobs)
        })
        .collect())
}

#[test]
fn configured_cap_reaches_every_task_of_that_crate() -> TestResult {
    let capped = build_jobs_by_task(&config("[stacks.rust.crates.demo]\nbuild_jobs = 1\n"))?;
    assert!(capped.len() >= 4, "demo has its full task set: {capped:?}");
    assert!(
        capped.values().all(|jobs| jobs.as_deref() == Some("1")),
        "every demo task is capped: {capped:?}"
    );
    Ok(())
}

#[test]
fn uncapped_crates_keep_cargo_defaults() -> TestResult {
    let plain = build_jobs_by_task(&config(""))?;
    assert!(!plain.is_empty());
    assert!(plain.values().all(Option::is_none), "{plain:?}");
    Ok(())
}

#[test]
fn unknown_package_fails_generation_closed() -> TestResult {
    let repo = make_repo(&config("[stacks.rust.crates.typo]\nbuild_jobs = 1\n"))?;
    let err = prepare(repo.path())
        .and_then(|prep| render_staged_tree(&prep))
        .expect_err("unknown package");
    assert!(err.to_string().contains("unknown_rust_package"), "{err}");
    Ok(())
}
