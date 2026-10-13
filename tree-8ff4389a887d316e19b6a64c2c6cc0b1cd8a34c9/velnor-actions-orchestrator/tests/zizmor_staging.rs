//! Staging-only zizmor config cases: zero-ignore staging config over full-SHA refs.

use crate::git_fixture;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use velnor_actions_actionlint::actions::{
    ALINT_ACTION, ALINT_ACTION_SHA, ALINT_ACTION_VERSION, CHECKOUT_ACTION_SHA,
    CHECKOUT_ACTION_VERSION,
};
use velnor_actions_actionlint::config::{
    ZizmorConfigInput, ZizmorWorkflowText, render_zizmor_yaml,
};
use velnor_actions_contract::FRESHNESS_WORKFLOW_PATH;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ProcessOutput, ToolCatalog};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

#[path = "fixture_package.rs"]
mod fixture_package;
#[path = "zizmor_staging_retired.rs"]
mod retired_foundation;

/// Test error shortcut.
type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run git with inherited failure context.
fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = git_fixture::command(cwd)?
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed");
    Ok(())
}

/// Release-manifest fixture for `prepare`.
fn manifest_json() -> String {
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
    .iter()
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-{target}\",\"sha256\":\"{}\"}}",
            "a".repeat(64)
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "a".repeat(40)
    )
}

/// One generator-lock binary record.
fn binary_record(target: &str) -> String {
    format!(
        "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"\n",
        "a".repeat(64)
    )
}

/// Generator-lock fixture for Velnor-policy `prepare`.
fn lock_text() -> String {
    let bins = binary_record("x86_64-unknown-linux-gnu") + &binary_record("aarch64-apple-darwin");
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"0.1.0\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64)
    )
}

/// Live version-policy mirror from the working tree.
fn repo_policy() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!(
        "{}/../../.velnor/version-policy.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    Ok(fs::read_to_string(path)?)
}

/// Git fixture: config plus one root crate (uncommitted).
fn make_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    fs::write(root.join(".velnor/release-manifest.json"), manifest_json())?;
    fs::write(
        root.join("Cargo.toml"),
        fixture_package::root_manifest(config),
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Velnor-policy fixture: origin plus lock and policy mirror.
fn make_policy_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
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
    fs::write(repo.path().join(".velnor/generator.lock"), lock_text())?;
    fs::write(
        repo.path().join(".velnor/version-policy.toml"),
        repo_policy()?,
    )?;
    Ok(repo)
}

/// Live dirs plus preview root, workflow bytes, validators, and staged inputs.
type PolicyPreview = (
    TempDir,
    TempDir,
    PathBuf,
    String,
    Vec<String>,
    Vec<ZizmorWorkflowText>,
);

/// Green Velnor-policy preview.
fn policy_preview() -> Result<PolicyPreview, Box<dyn std::error::Error>> {
    let repo = make_policy_repo()?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    assert_eq!(report.files_written.len(), 5, "five generated items");
    assert!(
        report
            .files_written
            .iter()
            .any(|path| path == FRESHNESS_WORKFLOW_PATH),
        "freshness probe emitted: {:?}",
        report.files_written
    );
    let workflow_paths = generated_workflow_paths(&report.files_written);
    let expected_paths = vec![WORKFLOW_PATH.to_owned(), FRESHNESS_WORKFLOW_PATH.to_owned()];
    assert_eq!(workflow_paths, expected_paths, "exact Velnor workflow set");
    assert!(
        !preview
            .join(".github/workflows/foundation-qualification.yml")
            .exists(),
        "Velnor generation omits the retired Foundation workflow"
    );
    let workflows = read_generated_workflows(&preview, &workflow_paths)?;
    assert!(
        workflows.iter().all(|workflow| !workflow
            .text
            .contains("8758d976a1b25eb387f48aa04ea86f57739b84cf")),
        "Velnor workflows contain no Foundation action pin"
    );
    let yaml = fs::read_to_string(preview.join(WORKFLOW_PATH))?;
    Ok((repo, parent, preview, yaml, report.validated_by, workflows))
}

/// Workflow paths emitted by generation, sorted for exact policy assertions.
fn generated_workflow_paths(files: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = files
        .iter()
        .filter(|path| path.starts_with(".github/workflows/"))
        .cloned()
        .collect();
    paths.sort();
    paths
}

/// Capture every generated workflow for staged validation.
fn read_generated_workflows(
    preview: &Path,
    paths: &[String],
) -> Result<Vec<ZizmorWorkflowText>, Box<dyn std::error::Error>> {
    let mut workflows = Vec::with_capacity(paths.len());
    for path in paths {
        workflows.push(ZizmorWorkflowText {
            path: path.clone(),
            text: fs::read_to_string(preview.join(path))?,
        });
    }
    Ok(workflows)
}

/// Scratch tree mirroring staged validation: `.github` plus config.
fn stage(
    preview: &Path,
    workflows: &[ZizmorWorkflowText],
    yaml: &str,
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    let mut staged_workflows = Vec::with_capacity(workflows.len());
    for workflow in workflows {
        let text = if workflow.path == WORKFLOW_PATH {
            yaml.to_owned()
        } else {
            workflow.text.clone()
        };
        fs::write(root.join(&workflow.path), &text)?;
        staged_workflows.push(ZizmorWorkflowText {
            path: workflow.path.clone(),
            text,
        });
    }
    fs::write(
        root.join(".github/actionlint.yaml"),
        fs::read(preview.join(".github/actionlint.yaml"))?,
    )?;
    let input = ZizmorConfigInput {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        workflows: staged_workflows,
    };
    fs::write(root.join(".zizmor.yml"), render_zizmor_yaml(&input)?.yaml)?;
    Ok(dir)
}

/// Run pinned zizmor over `dir` exactly like staged validation.
fn run_zizmor(dir: &Path) -> Result<ProcessOutput, Box<dyn std::error::Error>> {
    let catalog = ToolCatalog::pinned();
    let program = OsString::from("zizmor");
    let args = [
        "--offline",
        "--no-progress",
        "--color",
        "never",
        "--config",
        ".zizmor.yml",
        ".",
    ];
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Zizmor],
        &program,
        args.iter().map(OsString::from).collect(),
    )?;
    Ok(exec.command(&catalog)?.with_cwd(dir.to_path_buf()).run()?)
}

/// Combined zizmor streams for summary assertions.
fn streams(output: &ProcessOutput) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn velnor_policy_blessed_sha_validates_green() -> TestResult {
    let (_repo, _parent, preview, yaml, validated_by, _) = policy_preview()?;
    assert_eq!(
        validated_by,
        vec![
            "actionlint@1.7.12".to_owned(),
            "shellcheck@0.11.0".to_owned(),
            "zizmor@1.30.1".to_owned(),
        ]
    );
    let blessed = format!("uses: {ALINT_ACTION}@{ALINT_ACTION_SHA}");
    assert!(yaml.contains(&blessed), "blessed SHA rendered");
    assert!(
        !preview.join(".zizmor.yml").exists(),
        "no staging config in output"
    );
    assert!(
        !preview.join(".github/.zizmor.yml").exists(),
        "no config in generated tree"
    );
    Ok(())
}

/// The one `undocumented-permissions` finding (low, auditor/pedantic-only)
/// stays suppressed across every generated workflow. Zero ignores: every ref
/// is hash-pinned.
#[test]
fn staging_suppressions_stable_no_new() -> TestResult {
    let (_repo, _parent, preview, yaml, _, workflows) = policy_preview()?;
    let staged = stage(&preview, &workflows, &yaml)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(output.success, "staged config greens zizmor: {text}");
    assert!(
        !text.contains("ignored"),
        "SHA-pinned refs leave nothing ignored: {text}"
    );
    assert!(text.contains("1 suppressed"), "no new suppressions: {text}");
    Ok(())
}

#[test]
fn consumer_tree_unaffected() -> TestResult {
    let repo = make_repo("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n")?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    assert_eq!(report.files_written.len(), 4, "four consumer tree items");
    let workflow_paths = generated_workflow_paths(&report.files_written);
    assert_eq!(
        workflow_paths,
        vec![WORKFLOW_PATH.to_owned()],
        "consumer policy emits only CI"
    );
    assert!(
        !preview.join(".zizmor.yml").exists(),
        "no staging config in output"
    );
    let input = ZizmorConfigInput {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        workflows: read_generated_workflows(&preview, &workflow_paths)?,
    };
    assert!(
        render_zizmor_yaml(&input)?.approved_ignores.is_empty(),
        "consumer needs no ignore"
    );
    Ok(())
}

#[test]
fn different_unpinned_tag_still_fails() -> TestResult {
    let (_repo, _parent, preview, yaml, _, workflows) = policy_preview()?;
    let pinned = format!("actions/checkout@{CHECKOUT_ACTION_SHA}");
    let unpinned = format!("actions/checkout@{CHECKOUT_ACTION_VERSION}");
    let mutated = yaml.replacen(&pinned, &unpinned, 1);
    assert_ne!(mutated, yaml, "fixture still pins checkout");
    let staged = stage(&preview, &workflows, &mutated)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(!output.success, "unpinned checkout must fail: {text}");
    assert!(text.contains("unpinned-uses"), "rule fires: {text}");
    assert!(text.contains(&unpinned), "other ref flagged: {text}");
    Ok(())
}

#[test]
fn blessed_repo_tag_ref_still_fails() -> TestResult {
    let (_repo, _parent, preview, yaml, _, workflows) = policy_preview()?;
    let blessed = format!("{ALINT_ACTION}@{ALINT_ACTION_SHA}");
    let wrong = format!("{ALINT_ACTION}@{ALINT_ACTION_VERSION}");
    let mutated = yaml.replacen(&blessed, &wrong, 1);
    assert_ne!(mutated, yaml, "fixture still carries blessed SHA");
    let staged = stage(&preview, &workflows, &mutated)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(!output.success, "wrong alint tag must fail: {text}");
    assert!(text.contains("unpinned-uses"), "rule fires: {text}");
    assert!(text.contains(&wrong), "wrong ref flagged: {text}");
    Ok(())
}
