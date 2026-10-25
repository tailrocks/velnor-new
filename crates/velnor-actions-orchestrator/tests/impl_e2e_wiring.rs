//! End-to-end emitted-YAML wiring cases over tempdir fixtures.
//!
//! Runs the real `prepare` → `render_staged_tree` path and asserts on the
//! emitted workflow text: Mise setup precedes every static `mise` use, the
//! task template always carries setup, helper provisioning precedes every
//! staged-binary use, the Velnor policy carries deny plus machete, and every
//! step is named.

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::impl_common::{
    TestResult, config_with_branch, git, make_repo, without_ambient_identity,
};
use crate::impl_e2e_tools_save::{check_one_tools_saver_per_key, check_tools_save_shape};

#[path = "impl_e2e_wiring_preseed.rs"]
mod preseed_tests;
#[path = "impl_e2e_wiring_setup_tests.rs"]
mod setup_tests;

/// One parsed step: display name plus full step body.
#[derive(Clone)]
pub(crate) struct StepText {
    /// Step display name; empty when the entry carries no `name:`.
    pub(crate) name: String,
    /// First line plus every continuation line of the step.
    pub(crate) body: String,
}

/// One parsed job section in step order.
pub(crate) struct JobText {
    /// Job ID from the section header.
    pub(crate) id: String,
    /// Steps in render order.
    pub(crate) steps: Vec<StepText>,
}

/// Parse every job section plus its steps from workflow text.
fn parse_jobs(yaml: &str) -> Vec<JobText> {
    let mut jobs: Vec<JobText> = Vec::new();
    let mut in_jobs = false;
    let mut in_steps = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
        } else if in_jobs && is_job_header(line) {
            in_steps = false;
            jobs.push(JobText {
                id: header_id(line),
                steps: Vec::new(),
            });
        } else if in_jobs && line == "    steps:" {
            in_steps = true;
        } else if in_jobs && in_steps {
            push_step_line(&mut jobs, line);
        }
    }
    jobs
}

/// True for a two-space job section header (`  <id>:`).
fn is_job_header(line: &str) -> bool {
    line.len() > 3 && line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':')
}

/// Job ID from a validated section header.
fn header_id(line: &str) -> String {
    line.trim().trim_end_matches(':').to_owned()
}

/// Append one steps-block line to the current job's steps.
fn push_step_line(jobs: &mut [JobText], line: &str) {
    let Some(job) = jobs.last_mut() else {
        return;
    };
    if let Some(rest) = line.strip_prefix("      - ") {
        let name = rest
            .strip_prefix("name: ")
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_owned();
        job.steps.push(StepText {
            name,
            body: line.to_owned(),
        });
    } else if let Some(step) = job.steps.last_mut() {
        step.body.push('\n');
        step.body.push_str(line);
    }
}

/// True for shell steps invoking the `mise` program.
fn uses_mise(step: &StepText) -> bool {
    step.body.contains("run:") && step.body.contains("mise ")
}

/// Pre-seed staging step display name, asserted as emitted text.
const PRESEED_STAGE_TEXT: &str = "Stage helper (pre-seed trust-on-review)";

/// True for steps invoking the staged helper (excluding the stagers).
fn uses_staged_helper(step: &StepText) -> bool {
    step.name != "Acquire Velnor"
        && step.name != PRESEED_STAGE_TEXT
        && step.body.contains("$RUNNER_TEMP/velnor/bin")
}

/// Setup Mise precedes tool use after the permitted bootstrap prefix.
fn check_setup_first(job: &JobText) -> Result<(), String> {
    let setup = job.steps.iter().position(|s| s.name == "Setup Mise");
    let first_mise = job.steps.iter().position(uses_mise);
    match (setup, first_mise) {
        (Some(at), Some(first)) if at < first && setup_is_early(job, at) => Ok(()),
        (Some(_) | None, None) => Ok(()),
        (None, Some(_)) => Err(format!("{}: mise use without Setup Mise", job.id)),
        (Some(at), Some(first)) => Err(format!(
            "{}: Setup Mise at {at}, first mise use at {first}",
            job.id
        )),
    }
}

/// Setup position follows the supported cache bootstrap steps or the cold path.
fn setup_is_early(job: &JobText, at: usize) -> bool {
    let offset = usize::from(job.steps.first().is_some_and(is_cache_clock));
    let steps = &job.steps[offset..];
    let Some(at) = at.checked_sub(offset) else {
        return false;
    };
    let has_checkout = steps.first().is_some_and(|step| step.name == "Checkout");
    let restored_cache = steps.get(1).is_some_and(|step| step.name == "V2 identity")
        && steps.get(2).is_some_and(|step| {
            step.name == velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_NAME
        });
    let standard_v2 = at == 3 && has_checkout && restored_cache;
    let standard_bootstrap = steps
        .get(1)
        .is_some_and(|step| step.name == "Acquire Velnor")
        && steps
            .get(2)
            .is_some_and(|step| step.name == "Install cache tar adapters");
    let preseed_bootstrap = steps
        .get(1)
        .is_some_and(|step| step.name == "Download helper (pre-seed trust-on-review)")
        && steps
            .get(2)
            .is_some_and(|step| step.name == "Verify helper manifest (pre-seed trust-on-review)")
        && steps
            .get(3)
            .is_some_and(|step| step.name == "Stage helper (pre-seed trust-on-review)")
        && steps
            .get(4)
            .is_some_and(|step| step.name == "Install cache tar adapters");
    let standard_bootstrap_ready = at == 3 && has_checkout && standard_bootstrap;
    let preseed_bootstrap_ready = at == 5 && has_checkout && preseed_bootstrap;
    if steps.iter().any(is_cache_clock) {
        return false;
    }
    if job.id == "required" {
        let locked_prefix = ["Acquire Velnor", "Checkout"];
        let preseed_prefix = [
            "Download helper (pre-seed trust-on-review)",
            "Verify helper manifest (pre-seed trust-on-review)",
            "Stage helper (pre-seed trust-on-review)",
            "Checkout",
        ];
        return at == 1 && has_checkout
            || standard_v2
            || at == locked_prefix.len() && has_prefix(steps, &locked_prefix)
            || at == preseed_prefix.len() && has_prefix(steps, &preseed_prefix)
            || standard_bootstrap_ready
            || preseed_bootstrap_ready;
    }
    at <= 1 || standard_v2 || standard_bootstrap_ready || preseed_bootstrap_ready
}

fn has_prefix(steps: &[StepText], expected: &[&str]) -> bool {
    steps.len() >= expected.len()
        && steps
            .iter()
            .zip(expected)
            .all(|(step, name)| step.name == *name)
}

/// Only the renderer's reserved first-step clock may extend the bootstrap prefix.
fn is_cache_clock(step: &StepText) -> bool {
    step.name == "Record optional cache budget"
        && step.body.lines().any(|line| {
            matches!(
                line.trim(),
                "id: velnor-cache-clock" | "id: \"velnor-cache-clock\""
            )
        })
}

/// Setup Mise must leave restore and save ownership to the explicit V2 layer.
fn check_setup_cache_disabled(job: &JobText) -> Result<(), String> {
    for step in job.steps.iter().filter(|s| s.name == "Setup Mise") {
        for need in ["cache: \"false\"", "cache_save: \"false\""] {
            if !step.body.contains(need) {
                return Err(format!("{}: Setup Mise misses {need}", job.id));
            }
        }
        if step.body.contains("cache_save: ${{") {
            return Err(format!("{}: Setup Mise must not promise a save", job.id));
        }
        if step.body.contains("cache_key:") {
            return Err(format!("{}: Setup Mise must not own a tools key", job.id));
        }
    }
    Ok(())
}

/// Verify-MBX step must keep the executable check and prove the pinned route.
fn check_verify_mbx(plan: &JobText, mbx: &str) -> Result<(), String> {
    let verify = plan
        .steps
        .iter()
        .find(|step| step.name.contains("Verify MBX compile"))
        .ok_or("missing Verify MBX compile step")?;
    for need in [
        "test -x target/release/velnor-actions".to_owned(),
        "command -v mbx".to_owned(),
        "\\\"$mbx_bin\\\" --version".to_owned(),
        format!("grep -qxF \\\"mbx {mbx}\\\" \\\"$RUNNER_TEMP/velnor/preseed-mbx-version\\\""),
    ] {
        if !verify.body.contains(&need) {
            return Err(format!("verify misses {need}"));
        }
    }
    Ok(())
}

/// Every staged-helper use must follow an Acquire or pre-seed Stage step.
fn check_provisioned(job: &JobText) -> Result<(), String> {
    for (index, step) in job.steps.iter().enumerate() {
        if uses_staged_helper(step)
            && !job.steps[..index]
                .iter()
                .any(|prior| prior.name == "Acquire Velnor" || prior.name == PRESEED_STAGE_TEXT)
        {
            return Err(format!(
                "{}: step {:?} uses the staged helper without provisioning",
                job.id, step.name
            ));
        }
    }
    Ok(())
}

/// Every step entry must carry a display name.
fn check_named(job: &JobText) -> Result<(), String> {
    for step in &job.steps {
        if step.name.is_empty() {
            return Err(format!("{}: unnamed step: {:?}", job.id, step.body));
        }
    }
    Ok(())
}

/// Shared wiring checks for every job in one emitted tree.
fn check_tree(yaml: &str) -> Result<Vec<JobText>, String> {
    let jobs = parse_jobs(yaml);
    if jobs.is_empty() {
        return Err("no jobs parsed".to_owned());
    }
    for job in &jobs {
        check_setup_first(job)?;
        check_setup_cache_disabled(job)?;
        check_tools_save_shape(job)?;
        check_provisioned(job)?;
        check_named(job)?;
    }
    check_one_tools_saver_per_key(&jobs)?;
    Ok(jobs)
}

/// Velnor-policy fixture: canonical origin, lock optional (pre-seed omits it).
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

/// Three-target generator lock turning a Velnor fixture post-seed.
fn write_lock(repo: &tempfile::TempDir) -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let mut bins = String::new();
    for target in velnor_actions_contract::SUPPORTED_TARGETS {
        use std::fmt::Write as _;
        write!(
            bins,
            "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"\n",
            "a".repeat(64)
        )?;
    }
    let lock = format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"{mise_version}\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64),
        mise_version = velnor_actions_mise::MISE_VERSION,
    );
    std::fs::write(repo.path().join(".velnor/generator.lock"), lock)?;
    Ok(())
}

#[test]
fn emitted_yaml_wires_helpers_velnor_policy() -> TestResult {
    without_ambient_identity("emitted_yaml_wires_helpers_velnor_policy", || {
        let repo = make_velnor_repo()?;
        write_lock(&repo)?;
        let prep = prepare(repo.path())?;
        let tree = render_staged_tree(&prep)?;
        let yaml = tree
            .get(WORKFLOW_PATH)
            .ok_or("missing workflow in staged tree")?;
        assert!(
            yaml.contains("predecessor_run_attempt:"),
            "dispatch carries the exact predecessor attempt:\n{yaml}"
        );
        assert!(
            yaml.contains("predecessor_run_id:"),
            "dispatch carries the exact predecessor run:\n{yaml}"
        );
        let jobs = check_tree(yaml).map_err(|err| format!("{err}:\n{yaml}"))?;
        assert!(jobs.iter().all(|job| job.id != "policy"), "no umbrella");
        for (id, want) in [
            ("cargo-deny", "Run cargo-deny"),
            ("cargo-machete", "Run cargo-machete"),
        ] {
            let job = jobs.iter().find(|job| job.id == id);
            let job = job.unwrap_or_else(|| panic!("missing {id} job"));
            assert!(
                job.steps.iter().any(|step| step.name == want),
                "{id} misses {want}"
            );
        }
        assert!(
            jobs.iter().all(|job| job.id != "zizmor"),
            "zizmor is retired (hard decision 10)"
        );
        Ok(())
    })
}

#[test]
fn emitted_yaml_wires_helpers_consumer() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow in staged tree")?;
    let jobs = check_tree(yaml).map_err(|err| format!("{err}:\n{yaml}"))?;
    for id in ["alint", "cargo-deny", "cargo-machete", "zizmor"] {
        assert!(jobs.iter().all(|job| job.id != id), "consumer emits {id}");
    }
    Ok(())
}
