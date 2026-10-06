//! Plan-job `Prepare pinned tools` emission: install before generate.
//!
//! Regression coverage for the CI failure where `Check generated files`
//! ran `generate` before any `mise install`: the public `generate`
//! fail-closed-execs pinned actionlint/shellcheck/zizmor, so the emitted
//! plan job must install exact pins first.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{
    MBX_VERSION_CHECK_NAME, WORKFLOW_PATH, steps::MBX_RESTORE_NAME,
};

use crate::impl_common::{
    TestResult, config_with_branch, git, make_repo, without_ambient_identity,
};

/// Emitted plan-job ID.
const PLAN_JOB_ID: &str = "plan";

/// Step names plus bodies of the emitted plan job, in render order.
fn plan_steps(yaml: &str) -> Vec<(String, String)> {
    let mut steps = Vec::new();
    let mut in_plan = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_plan = false;
        } else if is_job_header(line) {
            in_plan = header_id(line) == PLAN_JOB_ID;
        } else if in_plan {
            push_step_line(&mut steps, line);
        }
    }
    steps
}

/// True for a two-space job section header (`  <id>:`).
fn is_job_header(line: &str) -> bool {
    line.len() > 3 && line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':')
}

/// Job ID from a validated section header.
fn header_id(line: &str) -> String {
    line.trim().trim_end_matches(':').to_owned()
}

/// Append one plan-job line to the current step, starting new steps on `- `.
fn push_step_line(steps: &mut Vec<(String, String)>, line: &str) {
    if let Some(rest) = line.strip_prefix("      - ") {
        let name = rest
            .strip_prefix("name: ")
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_owned();
        steps.push((name, line.to_owned()));
    } else if let Some((_, body)) = steps.last_mut() {
        body.push('\n');
        body.push_str(line);
    }
}

/// Position of one named step in the emitted plan job.
fn step_index(steps: &[(String, String)], name: &str) -> Option<usize> {
    steps.iter().position(|(step, _)| step == name)
}

/// Rendered workflow text for one fixture repo root.
fn workflow_yaml(root: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let prep = prepare(root)?;
    let tree = render_staged_tree(&prep)?;
    tree.get(WORKFLOW_PATH)
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("missing workflow in staged tree").into())
}

/// Install step must carry `mise install` plus every expected exact spec.
fn check_install_specs(body: &str, specs: &[String]) -> Result<(), String> {
    for need in ["mise ", "--no-config", "--no-env", "--no-hooks", "install"] {
        if !body.contains(need) {
            return Err(format!("install body misses {need}:\n{body}"));
        }
    }
    for spec in specs {
        if !body.contains(spec) {
            return Err(format!("install body misses {spec}:\n{body}"));
        }
    }
    Ok(())
}

/// Install step and job must carry the verification-plus-homes env.
fn check_install_env(body: &str, yaml: &str) -> Result<(), String> {
    for key in ["MISE_RUSTUP_HOME:", "MISE_CARGO_HOME:"] {
        if !body.contains(key) {
            return Err(format!("install step env misses {key}:\n{body}"));
        }
    }
    if !yaml.contains("RUSTUP_TOOLCHAIN: 1.98.1") {
        return Err(format!("job env misses RUSTUP_TOOLCHAIN:\n{yaml}"));
    }
    for key in ["MISE_NO_CONFIG:", "MISE_NO_ENV:", "MISE_NO_HOOKS:"] {
        if !yaml.contains(key) {
            return Err(format!("job env misses {key}:\n{yaml}"));
        }
    }
    if !yaml.contains("MISE_LOCKFILE: \"0\"") {
        return Err(format!("job env must pin MISE_LOCKFILE off:\n{yaml}"));
    }
    Ok(())
}

#[test]
fn consumer_plan_installs_validators_before_check_generated() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let yaml = workflow_yaml(repo.path())?;
    let steps = plan_steps(&yaml);
    let prepare_at = step_index(&steps, "Prepare pinned tools").ok_or("missing Prepare step")?;
    let check_at = step_index(&steps, "Check generated files").ok_or("missing Check step")?;
    assert!(
        prepare_at < check_at,
        "install must precede generate: {steps:?}"
    );
    let catalog = ToolCatalog::pinned();
    let specs = [
        PinnedTool::Rust,
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]
    .iter()
    .map(|tool| catalog.tool_spec(*tool))
    .collect::<Vec<_>>();
    let body = &steps[prepare_at].1;
    check_install_specs(body, &specs).map_err(|err| format!("{err}\n{yaml}"))?;
    check_install_env(body, &yaml).map_err(|err| format!("{err}\n{yaml}"))?;
    assert!(
        !body.contains("mr-boxington"),
        "cargo fixture must stay MBX-free:\n{body}"
    );
    Ok(())
}

#[test]
fn mbx_evidence_keeps_plan_mise_free_and_uses_task_action() -> TestResult {
    let repo = make_velnor_repo()?;
    let cargo_dir = repo.path().join(".cargo");
    std::fs::create_dir_all(&cargo_dir)?;
    std::fs::write(
        cargo_dir.join("config.toml"),
        "[build]\nrustc-wrapper = \"mbx\"\n",
    )?;
    let yaml = workflow_yaml(repo.path())?;
    let steps = plan_steps(&yaml);
    let prepare_at = step_index(&steps, "Prepare pinned tools").ok_or("missing Prepare step")?;
    let catalog = ToolCatalog::pinned();
    let spec = catalog.tool_spec(PinnedTool::MrBoxington);
    assert!(
        !steps[prepare_at].1.contains(&spec),
        "the non-compiling plan job must not install MBX:\n{}",
        steps[prepare_at].1
    );
    let task_start = yaml.find("  rust-demo:").ok_or("missing MBX task job")?;
    let task_tail = &yaml[task_start..];
    let mut task_end = 0;
    for line in task_tail.split_inclusive('\n') {
        if task_end > 0 && is_job_header(line.trim_end_matches('\n')) {
            break;
        }
        task_end += line.len();
    }
    let task = &task_tail[..task_end];
    let version = catalog.version(PinnedTool::MrBoxington);
    assert!(
        task.matches("uses: jdx/mr-boxington-action@").count() == 1
            && task.contains(&format!("version: {version}"))
            && task.contains(MBX_VERSION_CHECK_NAME),
        "the MBX task has one pinned native action and version guard:\n{task}"
    );
    assert!(
        !task.contains(&spec),
        "MBX is not duplicated through Mise:\n{task}"
    );
    assert_eq!(
        task.matches(MBX_RESTORE_NAME).count(),
        1,
        "one action-owner step:\n{task}"
    );
    Ok(())
}

#[test]
fn non_rust_consumer_plan_omits_rust_bootstrap_in_generated_yaml() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    std::fs::remove_file(repo.path().join("Cargo.toml"))?;
    std::fs::remove_dir_all(repo.path().join("src"))?;
    let yaml = workflow_yaml(repo.path())?;
    let steps = plan_steps(&yaml);
    let prepare_at = step_index(&steps, "Prepare pinned tools").ok_or("missing Prepare step")?;
    let prepare = &steps[prepare_at].1;
    let catalog = ToolCatalog::pinned();
    let rust = catalog.tool_spec(PinnedTool::Rust);
    assert!(
        !prepare.contains(&rust),
        "tools-only consumer must not install {rust}:\n{prepare}"
    );
    assert!(
        !steps
            .iter()
            .any(|(name, _)| name == "Prepare Rust components"),
        "tools-only consumer must omit Rust components: {steps:?}"
    );
    assert!(
        !steps
            .iter()
            .any(|(name, _)| name.contains("Cargo") || name.contains("MBX")),
        "tools-only consumer must omit Cargo and MBX setup: {steps:?}"
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(
            !steps.iter().any(|(_, body)| body.contains(key)),
            "tools-only plan must omit {key}: {steps:?}"
        );
    }
    for tool in [
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ] {
        let spec = catalog.tool_spec(tool);
        assert!(
            prepare.contains(&spec),
            "tools-only consumer must keep validator {spec}:\n{prepare}"
        );
    }
    Ok(())
}

/// Velnor-policy fixture: canonical origin, no lock (pre-seed shape).
fn make_velnor_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n",
    )?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        repo.path(),
    )?;
    Ok(repo)
}

/// `MISE_CARGO_HOME` value carried by one rendered step body, if any.
fn cargo_home_of(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        line.trim()
            .strip_prefix("MISE_CARGO_HOME:")
            .map(|value| value.trim().trim_matches('"').to_owned())
    })
}

#[test]
fn plan_fetch_check_and_plan_share_one_cargo_home() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    std::fs::write(
        repo.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    let yaml = workflow_yaml(repo.path())?;
    let steps = plan_steps(&yaml);
    let fetch_at = step_index(&steps, "Fetch Cargo sources").ok_or("missing Fetch step")?;
    let check_at = step_index(&steps, "Check generated files").ok_or("missing Check step")?;
    let plan_at = step_index(&steps, "Plan").ok_or("missing Plan step")?;
    let fetch_home = cargo_home_of(&steps[fetch_at].1).ok_or("fetch lacks cargo home")?;
    assert!(!fetch_home.is_empty(), "fetch home empty");
    for (name, at) in [("Check generated files", check_at), ("Plan", plan_at)] {
        let home = cargo_home_of(&steps[at].1).ok_or(format!("{name} lacks cargo home"))?;
        assert_eq!(
            home, fetch_home,
            "{name} must read the home Fetch populated (run 36754512444):\n{}",
            steps[at].1
        );
    }
    Ok(())
}

#[test]
fn preseed_plan_installs_before_build_and_check_generated() -> TestResult {
    without_ambient_identity(
        "preseed_plan_installs_before_build_and_check_generated",
        || {
            let repo = make_velnor_repo()?;
            let yaml = workflow_yaml(repo.path())?;
            let steps = plan_steps(&yaml);
            let prepare_at =
                step_index(&steps, "Prepare pinned tools").ok_or("missing Prepare step")?;
            let build_at = steps
                .iter()
                .position(|(step, _)| step.contains("Build helper"))
                .ok_or("missing Build helper step")?;
            let check_at =
                step_index(&steps, "Check generated files").ok_or("missing Check step")?;
            assert!(
                prepare_at < build_at && build_at < check_at,
                "install < build < generate: {steps:?}"
            );
            let rust = ToolCatalog::pinned().tool_spec(PinnedTool::Rust);
            assert!(steps[prepare_at].1.contains(&rust), "missing {rust}");
            // The pre-seed fixture proposes no Format task, so no plan
            // step consumes clippy/rustfmt: components stay uninstalled
            // (tied to consumers, see impl_cache_warm_components).
            assert!(
                step_index(&steps, "Prepare Rust components").is_none(),
                "fmt-less plan must not install unused components: {steps:?}"
            );
            Ok(())
        },
    )
}
