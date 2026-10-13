//! Gate-8 acquire and velnor-render-path cases.
use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::support::{TestResult, git, make_repo, without_ambient_identity};

fn binary_record(target: &str) -> String {
    format!(
        "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"\n",
        "a".repeat(64)
    )
}

fn lock_text() -> String {
    let bins = velnor_actions_contract_release::SUPPORTED_TARGETS
        .iter()
        .map(|target| binary_record(target))
        .collect::<String>();
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"0.1.0\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64)
    )
}

fn repo_policy() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!(
        "{}/../../../.velnor/version-policy.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    Ok(fs::read_to_string(path)?)
}

#[test]
fn consumer_plan_job_carries_acquire_step() -> TestResult {
    let repo = make_repo("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n")?;
    let prep = prepare(repo.path())?;
    let plan = prep.workflow.ir.jobs.get("plan").ok_or("no plan job")?;
    assert!(
        plan.steps.iter().any(|s| s.name == "Acquire Velnor"),
        "acquire wired"
    );
    Ok(())
}

#[test]
fn velnor_candidate_render_path_includes_release() -> TestResult {
    without_ambient_identity("velnor_candidate_render_path_includes_release", || {
        // Render path only: full `generate` runs staged validation.
        let repo = make_repo(
            "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ngenerator_validation = \"candidate\"\ndefault_branch = \"testmain\"\n[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"104c0d8b3a827875776358f941aa88f1c5837c1009305076af9380f4e3fcda25\"\nprofile = \"rust-strict-v1\"\n",
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
        fs::write(repo.path().join(".velnor/generator.lock"), lock_text())?;
        fs::write(
            repo.path().join(".velnor/version-policy.toml"),
            repo_policy()?,
        )?;
        let prep = prepare(repo.path())?;
        let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
            &prep.workflow.ir,
            prep.config.workflow.policy,
            prep.workflow.support.as_ref(),
            &prep.workflow.context,
        )
        .map_err(|err| format!("render: {err}"))?;
        for want in [
            "candidate:",
            "release:",
            "ref_protected",
            "Upload candidate",
            "Download candidate",
            "Qualify candidate",
        ] {
            assert!(yaml.contains(want), "missing {want}");
        }
        for want in ["alint:", "cargo-deny:", "cargo-machete:", "zizmor:"] {
            assert!(yaml.contains(want), "validator {want}");
        }
        Ok(())
    })
}

#[test]
fn version_policy_drift_fails_velnor_generate() -> TestResult {
    without_ambient_identity("version_policy_drift_fails_velnor_generate", || {
        let repo = make_repo(
            "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n[stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"104c0d8b3a827875776358f941aa88f1c5837c1009305076af9380f4e3fcda25\"\nprofile = \"rust-strict-v1\"\n",
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
        fs::write(
            repo.path().join(".velnor/version-policy.toml"),
            repo_policy()?.replace("1.98.1", "9.9.9"),
        )?;
        let prep = prepare(repo.path())?;
        let parent = TempDir::new()?;
        let err = generate(
            &prep,
            &GenerateOptions {
                output_dir: Some(parent.path().join("p")),
            },
        );
        assert!(err.is_err_and(|err| err.to_string().contains("tool:rust")));
        Ok(())
    })
}
