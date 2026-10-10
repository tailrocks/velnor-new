//! Staging-only zizmor config cases: zero-ignore staging config over full-SHA refs.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_actionlint::actions::{
    ALINT_ACTION, ALINT_ACTION_SHA, ALINT_ACTION_VERSION, CHECKOUT_ACTION_SHA,
    CHECKOUT_ACTION_VERSION,
};
use velnor_actions_actionlint::config::{
    ZizmorConfigInput, ZizmorWorkflowText, render_zizmor_yaml,
};
use velnor_actions_contract_workflow::FRESHNESS_WORKFLOW_PATH;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ProcessOutput, ToolCatalog};
use velnor_actions_orchestrator_generation::generate::{GenerateOptions, generate};
use velnor_actions_orchestrator_generation::prepare::prepare;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

/// Test error shortcut.
type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run git with inherited failure context.
fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = Command::new("git").args(args).current_dir(cwd).status()?;
    assert!(status.success(), "git {args:?} failed");
    Ok(())
}

/// Release-manifest fixture for `prepare`.
fn manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract_release::SUPPORTED_TARGETS
    .iter()
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
            "a".repeat(64)
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
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
    let version = env!("CARGO_PKG_VERSION");
    let bins = velnor_actions_contract_release::SUPPORTED_TARGETS
        .iter()
        .map(|target| binary_record(target))
        .collect::<String>();
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64)
    )
}

/// Live version-policy mirror from the working tree.
fn repo_policy() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!(
        "{}/../../../.velnor/version-policy.toml",
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
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Velnor-policy fixture: origin plus lock and policy mirror.
fn make_policy_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
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
    fs::write(repo.path().join(".velnor/generator.lock"), lock_text())?;
    fs::write(
        repo.path().join(".velnor/version-policy.toml"),
        repo_policy()?,
    )?;
    Ok(repo)
}

/// Live dirs plus preview root, workflow bytes, and validators.
type PolicyPreview = (TempDir, TempDir, PathBuf, String, Vec<String>);

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
    assert_eq!(report.files_written.len(), 7, "seven generated files");
    assert!(
        report
            .files_written
            .iter()
            .any(|path| path == FRESHNESS_WORKFLOW_PATH),
        "freshness probe emitted: {:?}",
        report.files_written
    );
    let yaml = fs::read_to_string(preview.join(WORKFLOW_PATH))?;
    Ok((repo, parent, preview, yaml, report.validated_by))
}

/// Scratch tree mirroring staged validation: `.github` plus config.
fn stage(preview: &Path, yaml: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    fs::write(root.join(WORKFLOW_PATH), yaml)?;
    fs::write(
        root.join(".github/actionlint.yaml"),
        fs::read(preview.join(".github/actionlint.yaml"))?,
    )?;
    let freshness = fs::read_to_string(preview.join(FRESHNESS_WORKFLOW_PATH))?;
    fs::write(root.join(FRESHNESS_WORKFLOW_PATH), &freshness)?;
    let action = ".github/actions/velnor-tool-seed/action.yml";
    fs::create_dir_all(root.join(".github/actions/velnor-tool-seed"))?;
    fs::copy(preview.join(action), root.join(action))?;
    let task_action = ".github/actions/task-rust-demo/action.yml";
    fs::create_dir_all(root.join(".github/actions/task-rust-demo"))?;
    fs::copy(preview.join(task_action), root.join(task_action))?;
    let input = ZizmorConfigInput {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        workflows: vec![
            ZizmorWorkflowText {
                path: WORKFLOW_PATH.to_owned(),
                text: yaml.to_owned(),
            },
            ZizmorWorkflowText {
                path: FRESHNESS_WORKFLOW_PATH.to_owned(),
                text: freshness,
            },
        ],
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
    let (_repo, _parent, preview, yaml, validated_by) = policy_preview()?;
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

/// The two suppressed findings are `undocumented-permissions` (low,
/// auditor/pedantic-only): Plan and Required each grant Actions read to their
/// bounded internal baseline/artifact operation. Inline ignores are only
/// `self-repository` on local actions. Zizmor must report that count.
#[test]
fn staging_suppressions_stable_no_new() -> TestResult {
    let (_repo, _parent, preview, yaml, _) = policy_preview()?;
    let staged = stage(&preview, &yaml)?;
    let ignores = self_repository_ignores(staged.path())?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(output.success, "staged config greens zizmor: {text}");
    assert!(ignores > 0, "the tool-seed action needs one ignore");
    assert!(
        text.contains(&format!("{ignores} ignored")),
        "ignore count must match the local-action annotations: {text}"
    );
    assert!(text.contains("2 suppressed"), "no new suppressions: {text}");
    Ok(())
}

/// Count `# zizmor: ignore[self-repository]` and reject every other ignore.
fn self_repository_ignores(root: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let mut matched = 0;
    let mut any = 0;
    for relative in [
        WORKFLOW_PATH,
        FRESHNESS_WORKFLOW_PATH,
        ".github/actions/velnor-tool-seed/action.yml",
        ".github/actions/task-rust-demo/action.yml",
    ] {
        let text = fs::read_to_string(root.join(relative))?;
        matched += text.matches("# zizmor: ignore[self-repository]").count();
        any += text.matches("zizmor: ignore[").count();
    }
    assert_eq!(matched, any, "only self-repository ignores are allowed");
    Ok(matched)
}

#[test]
fn consumer_tree_unaffected() -> TestResult {
    let repo = make_repo("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n")?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    assert!(
        !preview.join(".zizmor.yml").exists(),
        "no staging config in output"
    );
    let yaml = fs::read_to_string(preview.join(WORKFLOW_PATH))?;
    let input = ZizmorConfigInput {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        workflows: vec![ZizmorWorkflowText {
            path: WORKFLOW_PATH.to_owned(),
            text: yaml,
        }],
    };
    assert!(
        render_zizmor_yaml(&input)?.approved_ignores.is_empty(),
        "consumer needs no ignore"
    );
    Ok(())
}

#[test]
fn different_unpinned_tag_still_fails() -> TestResult {
    let (_repo, _parent, preview, yaml, _) = policy_preview()?;
    let pinned = format!("actions/checkout@{CHECKOUT_ACTION_SHA}");
    let unpinned = format!("actions/checkout@{CHECKOUT_ACTION_VERSION}");
    let mutated = yaml.replacen(&pinned, &unpinned, 1);
    assert_ne!(mutated, yaml, "fixture still pins checkout");
    let staged = stage(&preview, &mutated)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(!output.success, "unpinned checkout must fail: {text}");
    assert!(text.contains("unpinned-uses"), "rule fires: {text}");
    assert!(text.contains(&unpinned), "other ref flagged: {text}");
    Ok(())
}

#[test]
fn blessed_repo_tag_ref_still_fails() -> TestResult {
    let (_repo, _parent, preview, yaml, _) = policy_preview()?;
    let blessed = format!("{ALINT_ACTION}@{ALINT_ACTION_SHA}");
    let wrong = format!("{ALINT_ACTION}@{ALINT_ACTION_VERSION}");
    let mutated = yaml.replacen(&blessed, &wrong, 1);
    assert_ne!(mutated, yaml, "fixture still carries blessed SHA");
    let staged = stage(&preview, &mutated)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(!output.success, "wrong alint tag must fail: {text}");
    assert!(text.contains("unpinned-uses"), "rule fires: {text}");
    assert!(text.contains(&wrong), "wrong ref flagged: {text}");
    Ok(())
}
