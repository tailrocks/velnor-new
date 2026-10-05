//! Gate 1: pure-Tofu jobs install no Rust and use one private Mise home.

use super::*;

/// A pure-Tofu consumer installs no Rust toolchain or Cargo tools.
#[test]
fn gate1_pure_tofu_installs_no_rust_toolchain() -> TestResult {
    let dir = tofu_repo(2)?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let catalog = ToolCatalog::pinned();
    let rust = catalog.tool_spec(PinnedTool::Rust);
    let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
    let opentofu = catalog.tool_spec(PinnedTool::Opentofu);
    let mut checked = 0;
    for (id, job) in &jobs {
        if id != PLAN_JOB_ID && !is_crate_job(id) {
            continue;
        }
        checked += 1;
        let steps = names(job);
        assert!(
            !steps.contains(&"Prepare Rust components"),
            "{id} has no components step: {steps:?}"
        );
        assert!(
            !steps.iter().any(|name| name.starts_with("Fetch Cargo")),
            "{id} has no cargo fetch: {steps:?}"
        );
        for name in &steps {
            for banned in [
                "Rust", "Cargo", "rustfmt", "Clippy", "Nextest", "MBX", "metadata",
            ] {
                assert!(!name.contains(banned), "{id} step {name} names {banned}");
            }
        }
        let (run, env) = shell_of(job, "Prepare pinned tools")?;
        assert!(run.contains(&opentofu), "{id} installs opentofu: {run:?}");
        assert!(!run.contains(&rust), "{id} installs no Rust: {run:?}");
        assert!(!run.contains(&mbx), "{id} installs no MBX: {run:?}");
        assert_eq!(
            env.get("MISE_DATA_DIR").map(String::as_str),
            Some("${{ github.workspace }}/.velnor/cache/mise"),
            "{id} uses the private Mise data home"
        );
        for key in [
            "MISE_RUSTUP_HOME",
            "MISE_CARGO_HOME",
            "RUSTUP_HOME",
            "CARGO_HOME",
            "RUSTUP_TOOLCHAIN",
        ] {
            assert!(!env.contains_key(key), "{id} prepare carries no {key}");
        }
    }
    assert_eq!(checked, 3, "plan plus two stack jobs");
    Ok(())
}
