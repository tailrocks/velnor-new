//! T24 tofu perf fixtures: N-root synthetic repos + matrix case builders.
//!
//! Included via `#[path]` from `impl_tofu_t24_gates`, so the bench and
//! scale suites reuse it through that parent (the P13
//! `perf_fixtures_p13` precedent: one owner, `crate::` sharing).

use std::fs;
use std::path::Path;
use std::time::Instant;

use tempfile::TempDir;
use velnor_actions_contract_workflow::Plan;
use velnor_actions_orchestrator::plan_internal;

use crate::support::{TestResult, fixture_manifest_json, git, git_line};

/// Tofu config over `roots` with 2-wide staging.
pub(crate) fn tofu_config_for(roots: &[String]) -> String {
    let list = roots
        .iter()
        .map(|root| format!("\"{root}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
         max_parallel_jobs = 2\n[stacks.tofu]\nroots = [{list}]\n"
    )
}

/// Root names `stacks/r000` .. `stacks/r{roots:03}`.
pub(crate) fn tofu_root_names(roots: usize) -> Vec<String> {
    (0..roots)
        .map(|index| format!("stacks/r{index:03}"))
        .collect()
}

/// Write `.velnor` inputs: config plus the consumer-manifest fixture.
fn write_velnor(root: &Path, config: &str) -> TestResult {
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    Ok(())
}

/// Init a git repo with identity config (plan needs commits).
fn git_init(root: &Path) -> TestResult {
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    Ok(())
}

/// One tofu root with a single-variable file.
fn write_root(root: &Path, name: &str) -> TestResult {
    let dir = root.join(name);
    fs::create_dir_all(&dir)?;
    let leaf = name.rsplit('/').next().unwrap_or(name);
    fs::write(dir.join("main.tf"), format!("variable \"{leaf}\" {{}}\n"))?;
    Ok(())
}

/// Pure-tofu repo with `roots` disjoint roots (the `workspace_repo`
/// pattern for tofu: parameterized width, git-initialized, uncommitted).
///
/// Every repo also carries `README.md` so docs-only cases have a
/// non-tofu file to touch without inventing a second builder.
pub(crate) fn tofu_repo(roots: usize) -> Result<TempDir, Box<dyn std::error::Error>> {
    assert!(roots >= 1, "at least one root");
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    let names = tofu_root_names(roots);
    write_velnor(root, &tofu_config_for(&names))?;
    for name in &names {
        write_root(root, name)?;
    }
    fs::write(root.join("README.md"), "# demo\n")?;
    Ok(dir)
}

/// Pure-tofu repo whose root `.` calls one shared local module.
pub(crate) fn tofu_repo_with_module() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    write_velnor(
        root,
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
         [stacks.tofu]\nroots = [\".\"]\n",
    )?;
    fs::write(
        root.join("main.tf"),
        "module \"shared\" {\n  source = \"./mods/shared\"\n}\n",
    )?;
    fs::create_dir_all(root.join("mods/shared"))?;
    fs::write(root.join("mods/shared/main.tf"), "variable \"shared\" {}\n")?;
    fs::write(root.join("README.md"), "# demo\n")?;
    Ok(dir)
}

/// Minimal lockfile bytes so the fixture root counts as lockful.
pub(crate) fn demo_lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// Pure-tofu repo with one lockful root.
pub(crate) fn tofu_repo_with_lock() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    write_velnor(root, &tofu_config_for(&["stacks/a".to_owned()]))?;
    write_root(root, "stacks/a")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), demo_lock("a"))?;
    fs::write(root.join("README.md"), "# demo\n")?;
    Ok(dir)
}

/// Mixed repo: root Cargo crate plus `roots` tofu roots.
pub(crate) fn mixed_repo(roots: usize) -> Result<TempDir, Box<dyn std::error::Error>> {
    assert!(roots >= 1, "at least one root");
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    let names = tofu_root_names(roots);
    write_velnor(root, &tofu_config_for(&names))?;
    for name in &names {
        write_root(root, name)?;
    }
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    fs::write(root.join("README.md"), "# demo\n")?;
    Ok(dir)
}

/// Commit everything twice, appending `touch_body` to `touch_rel` for
/// the second commit. Returns `(base, head)` shas for planning.
pub(crate) fn commit_two_tofu(
    root: &Path,
    touch_rel: &str,
    touch_body: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let touch = root.join(touch_rel);
    let mut body = fs::read_to_string(&touch)?;
    body.push_str(touch_body);
    fs::write(&touch, body)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    Ok((base, head))
}

/// Plan `head` against `base` under one event; validated plan plus raw JSON.
pub(crate) fn plan_at_event(
    root: &Path,
    base: &str,
    head: &str,
    event: &str,
) -> Result<(Plan, String), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": event,
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((plan, response))
}

/// Direct `git ls-files` wall in ms: a lower-bound proxy for the
/// index-subprocess cost inside `plan` (the tofu analogue of the P13
/// direct-`cargo metadata` proxy; plan itself adds the untracked pass).
pub(crate) fn index_baseline_ms(root: &Path) -> Result<u128, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err("direct git ls-files failed".into());
    }
    Ok(start.elapsed().as_millis())
}
