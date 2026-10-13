//! Gate-8 acquire and velnor-render-path cases.
use std::fs;

use tempfile::TempDir;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, git, make_repo, without_ambient_identity};

fn binary_record(target: &str) -> String {
    format!(
        "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"\n",
        "a".repeat(64)
    )
}

fn lock_text() -> String {
    let bins = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| binary_record(target))
        .collect::<String>();
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"0.1.0\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.10.7\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64)
    )
}

fn repo_policy() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!(
        "{}/../../.velnor/version-policy.toml",
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
        // Render the validated output tree without writing it. This includes
        // the staged helper that declared task actions require.
        let repo = make_repo(
            "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ngenerator_validation = \"candidate\"\ndefault_branch = \"testmain\"\n",
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
        let tree = render_staged_tree(&prep)?;
        let yaml = tree
            .get(WORKFLOW_PATH)
            .ok_or("missing generated workflow")?;
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
fn velnor_manifest_skip_keeps_generator_lock_validation() -> TestResult {
    without_ambient_identity(
        "velnor_manifest_skip_keeps_generator_lock_validation",
        || {
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
            fs::write(
                repo.path().join(".velnor/release-manifest.json"),
                "not json",
            )?;
            fs::write(repo.path().join(".velnor/generator.lock"), "{nope")?;

            let prep = prepare(repo.path())?;
            assert_eq!(prep.discovery.consumer_manifest_json, None);
            let err = render_staged_tree(&prep)
                .err()
                .ok_or("malformed generator lock was accepted")?;
            assert!(
                err.to_string().contains("malformed_lock_toml"),
                "unexpected producer lock error: {err}"
            );
            Ok(())
        },
    )
}

#[test]
fn version_policy_drift_fails_velnor_generate() -> TestResult {
    without_ambient_identity("version_policy_drift_fails_velnor_generate", || {
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
        let policy = repo_policy()?;
        let catalog = ToolCatalog::pinned();
        let rust_version = catalog.version(PinnedTool::Rust);
        let drifted_policy =
            policy.replace(&format!("rust = \"{rust_version}\""), "rust = \"9.9.9\"");
        assert_ne!(
            drifted_policy, policy,
            "fixture tracks the catalog Rust pin"
        );
        fs::write(
            repo.path().join(".velnor/version-policy.toml"),
            drifted_policy,
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
