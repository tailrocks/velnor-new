//! Plan-job Cargo fetch cases: lockful sources fetch before generate
//! consumers, lockless workspaces fetch nothing, nested locks get names.

use std::fs;

use velnor_actions_contract_workflow::StepKind;
use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// Hand-written lock for a no-deps fixture package: hermetic, no network.
fn demo_lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

#[test]
fn plan_job_fetches_lockful_sources_before_generate_consumers() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    let prep = prepare(root)?;
    let plan = prep
        .workflow
        .ir
        .jobs
        .get("plan")
        .ok_or_else(|| std::io::Error::other("missing plan job"))?;
    let names: Vec<&str> = plan.steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|seen| *seen == name);
    // Cargo-only fixture: the writer is one `rust-cache` step (its post
    // action saves; no separate save step). MBX repos use restore/save.
    let (Some(prepare_at), Some(cache_at), Some(fetch_at), Some(write_at)) = (
        at("Prepare pinned tools"),
        at("Restore Cargo registry"),
        at("Fetch Cargo sources"),
        at("Write request"),
    ) else {
        return Err(format!("plan steps miss fetch ordering: {names:?}").into());
    };
    assert!(
        prepare_at < cache_at && cache_at < fetch_at && fetch_at < write_at,
        "cache<fetch<request: {names:?}"
    );
    let StepKind::Shell { run, env } = &plan.steps[fetch_at].kind else {
        return Err("fetch step must be a shell step".into());
    };
    assert_eq!(&run[..2], ["sh", "-c"]);
    for need in [
        "metadata --locked --offline",
        "cargo fetch --locked",
        "sources miss (source_missing)",
    ] {
        assert!(run[2].contains(need), "fetch script misses {need}");
    }
    assert!(
        env.get("MISE_CARGO_HOME").is_some_and(|v| !v.is_empty()),
        "writer fetches to owned homes"
    );
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    let fetch_pos = yaml
        .find("Fetch Cargo sources")
        .ok_or_else(|| std::io::Error::other("rendered fetch missing"))?;
    let check_pos = yaml
        .find("Check generated files")
        .ok_or_else(|| std::io::Error::other("rendered freshness missing"))?;
    let ordered = fetch_pos < check_pos;
    assert!(ordered, "fetch must precede Check generated files");
    Ok(())
}

#[test]
fn plan_job_omits_fetch_without_lockfile() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let plan = prep
        .workflow
        .ir
        .jobs
        .get("plan")
        .ok_or_else(|| std::io::Error::other("missing plan job"))?;
    assert!(
        plan.steps
            .iter()
            .all(|step| !step.name.starts_with("Fetch Cargo sources")),
        "lockless workspaces have nothing to fetch"
    );
    Ok(())
}

#[test]
fn nested_lockful_workspace_gets_named_fetch() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    fs::create_dir_all(root.join("nested/src"))?;
    fs::write(
        root.join("nested/Cargo.toml"),
        "[package]\nname = \"nested\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("nested/src/lib.rs"), "pub fn g() {}\n")?;
    fs::write(root.join("nested/Cargo.lock"), demo_lock("nested"))?;
    let prep = prepare(root)?;
    let plan = prep
        .workflow
        .ir
        .jobs
        .get("plan")
        .ok_or_else(|| std::io::Error::other("missing plan job"))?;
    let mut nested = plan.steps.iter().filter(|step| {
        matches!(&step.kind, StepKind::Shell { .. })
            && step.name == "Fetch Cargo sources (nested/Cargo.toml)"
    });
    let step = nested
        .next()
        .ok_or_else(|| std::io::Error::other("missing nested fetch"))?;
    assert!(nested.next().is_none(), "exactly one nested fetch");
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err("fetch step must be a shell step".into());
    };
    assert!(
        run[2].contains("--manifest-path \"$GITHUB_WORKSPACE/nested/Cargo.toml\""),
        "nested fetch names its absolute manifest: {run:?}"
    );
    assert!(
        plan.steps
            .iter()
            .any(|step| step.name == "Fetch Cargo sources"),
        "root fetch still present"
    );
    Ok(())
}
