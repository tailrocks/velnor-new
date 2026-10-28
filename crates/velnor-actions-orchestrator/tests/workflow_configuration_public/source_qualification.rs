// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_contract::{
    RESERVED_SUITE_WORKFLOW_SLUGS, RustSuiteConfig, validate_preserve_workflows,
    validate_rust_suites,
};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::MARKER_PREFIX;

use super::workflow_configuration_fixture;

const WORKFLOW_PATH: &str = ".github/workflows/source-qualification.yml";
const UNRELATED_PATH: &str = ".github/ISSUE_TEMPLATE/config.yml";
const UNRELATED_CONTENT: &str = "name: preserved user content\n";

#[test]
fn generator_release_owns_source_qualification_lifecycle() -> Result<(), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let root = repo.path();
    let source_workflow = root.join(WORKFLOW_PATH);
    fs::create_dir_all(
        source_workflow
            .parent()
            .ok_or("workflow path has no parent")?,
    )?;
    fs::write(&source_workflow, "name: stale user-owned workflow\n")?;
    fs::create_dir_all(
        root.join(UNRELATED_PATH)
            .parent()
            .ok_or("unrelated path has no parent")?,
    )?;
    fs::write(root.join(UNRELATED_PATH), UNRELATED_CONTENT)?;

    let prepared = prepare(root)?;
    let tree = render_staged_tree(&prepared)?;
    let generated = tree
        .get(WORKFLOW_PATH)
        .ok_or("GeneratorRelease did not render its source qualification companion")?;
    assert!(generated.starts_with(MARKER_PREFIX));
    let rendered = generated;
    assert!(rendered.contains("pull_request: {}"));
    assert!(rendered.contains("workflow_dispatch: {}"));
    assert!(!rendered.contains("push:"));
    assert!(!rendered.contains("schedule:"));
    assert!(!rendered.contains("workflow_run:"));
    assert!(rendered.contains("permissions:\n  contents: read"));
    assert!(!rendered.contains("contents: write"));
    assert!(!rendered.contains("stale user-owned workflow"));
    assert_manual_native_validator_step(rendered)?;

    let previews = tempfile::tempdir()?;
    let first = preview_generation(&prepared, previews.path().join("enabled-first"))?;
    let second = preview_generation(&prepared, previews.path().join("enabled-second"))?;
    let first_workflow = fs::read(
        first.join(".github").join(
            WORKFLOW_PATH
                .strip_prefix(".github/")
                .ok_or("workflow path prefix")?,
        ),
    )?;
    let second_workflow = fs::read(
        second.join(".github").join(
            WORKFLOW_PATH
                .strip_prefix(".github/")
                .ok_or("workflow path prefix")?,
        ),
    )?;
    assert_eq!(first_workflow, second_workflow);
    let unrelated = Path::new(".github").join(
        UNRELATED_PATH
            .strip_prefix(".github/")
            .ok_or("unrelated path prefix")?,
    );
    assert_eq!(
        fs::read(first.join(&unrelated))?,
        UNRELATED_CONTENT.as_bytes()
    );
    assert_eq!(
        fs::read(second.join(&unrelated))?,
        UNRELATED_CONTENT.as_bytes()
    );

    fs::write(root.join(".velnor/config.toml"), generator_config(false))?;
    let disabled = prepare(root)?;
    let retired = preview_generation(&disabled, previews.path().join("disabled"))?;
    assert!(
        !retired
            .join(".github")
            .join(
                WORKFLOW_PATH
                    .strip_prefix(".github/")
                    .ok_or("workflow path prefix")?
            )
            .exists()
    );
    assert_eq!(
        fs::read(retired.join(&unrelated))?,
        UNRELATED_CONTENT.as_bytes()
    );
    Ok(())
}

#[test]
fn source_qualification_reservations_reject_collisions_and_allow_unrelated_entries() {
    let preserve_error =
        validate_preserve_workflows(&[WORKFLOW_PATH.to_owned()], ".velnor/config.toml")
            .expect_err("generator-owned workflow must reject preserve_workflows collision");
    assert!(
        preserve_error
            .to_string()
            .contains("cannot_preserve_generator_workflow")
    );
    validate_preserve_workflows(
        &[".github/workflows/project-checks.yml".to_owned()],
        ".velnor/config.toml",
    )
    .expect("unrelated workflow may remain preserved");

    assert!(RESERVED_SUITE_WORKFLOW_SLUGS.contains(&"source-qualification"));
    let mut suite = RustSuiteConfig::new("project-check", "default");
    suite.workflow = Some("source-qualification".to_owned());
    let suite_error = validate_rust_suites(&[suite], ".velnor/config.toml")
        .expect_err("generator-owned workflow slug must reject suite collision");
    assert!(
        suite_error
            .to_string()
            .contains("reserved_suite_workflow:source-qualification")
    );

    let mut unrelated = RustSuiteConfig::new("project-check", "default");
    unrelated.workflow = Some("project-checks".to_owned());
    validate_rust_suites(&[unrelated], ".velnor/config.toml")
        .expect("unrelated suite workflow slug must remain valid");
}

fn generator_repo(enabled: bool) -> Result<TempDir, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(&generator_config(enabled))?;
    Ok(repo)
}

fn assert_manual_native_validator_step(rendered: &str) -> Result<(), Box<dyn Error>> {
    let build_step = rendered
        .find("- name: Build source CLI with pinned MBX")
        .ok_or("source CLI build step was not rendered")?;
    let qualification_step = rendered
        .find("- name: Author Linux x64 native validator candidates")
        .ok_or("manual native validator qualification step was not rendered")?;
    let capture_step = rendered
        .find("- name: Capture all five external goldens")
        .ok_or("external golden capture step was not rendered")?;
    assert!(build_step < qualification_step && qualification_step < capture_step);
    assert_eq!(
        rendered
            .matches("- name: Author Linux x64 native validator candidates")
            .count(),
        1
    );
    let manual_qualification = &rendered[qualification_step..capture_step];
    assert!(
        manual_qualification.contains("if: success() && github.event_name == 'workflow_dispatch'")
    );
    assert!(manual_qualification.contains(
        "run: bash scripts/generator-release/source-qualification.sh qualify-linux-x64-native-validator-tools"
    ));
    Ok(())
}

fn generator_config(enabled: bool) -> String {
    let workflows = if enabled {
        "workflows = [\"generator_release\"]\n"
    } else {
        "workflows = []\n"
    };
    format!(
        "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n{workflows}\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
    )
}

fn preview_generation(
    preparation: &velnor_actions_orchestrator::GenerationPreparation,
    destination: PathBuf,
) -> Result<PathBuf, Box<dyn Error>> {
    generate(
        preparation,
        &GenerateOptions {
            output_dir: Some(destination.clone()),
        },
    )?;
    Ok(destination)
}

const REQUIRED_STATUSES: &[&str] = &[
    "cargo-prefetch",
    "archive-guard-build",
    "validator-tool-install",
    "validator-tool-lock-admission",
    "cli-build",
    "external-capture",
    "external-check",
    "rust-verifier-retry",
    "external-shell-harness",
    "external-shell-harness-keep",
    "external-shell-harness-json-proof",
    "repository-oracle-check",
];

#[test]
fn phase_dispatch_rejects_unknown_and_extra_arguments_before_side_effects()
-> Result<(), Box<dyn Error>> {
    let temporary = tempfile::tempdir()?;
    let unknown = run_phase(temporary.path(), &["unknown-phase"])?;
    assert_eq!(unknown.status.code(), Some(64));
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("unknown source qualification phase")
    );
    assert!(
        !temporary
            .path()
            .join("source-qualification-artifacts")
            .exists()
    );

    let extra = run_phase(
        temporary.path(),
        &["prepare-external-evidence-directories", "unexpected"],
    )?;
    assert_eq!(extra.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&extra.stderr).contains("usage:"));
    assert!(
        !temporary
            .path()
            .join("source-qualification-artifacts")
            .exists()
    );

    Ok(())
}

#[test]
fn aggregation_accepts_complete_success_receipts_and_retains_result() -> Result<(), Box<dyn Error>>
{
    let temporary = tempfile::tempdir()?;
    write_receipts(temporary.path(), None, None)?;
    let output = run_phase(
        temporary.path(),
        &["bundle-selected-qualification-evidence"],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = evidence_dir(temporary.path());
    assert_eq!(
        fs::read_to_string(evidence.join("qualification-final-status.txt"))?,
        "qualification_checks_status=0\n"
    );
    assert_eq!(
        fs::read_to_string(evidence.join("logs/evidence-bundle.exit"))?,
        "0\n"
    );
    assert!(
        evidence
            .join("source-qualification-preserved.tar.gz")
            .is_file()
    );
    Ok(())
}

#[test]
fn aggregation_fails_and_retains_status_for_missing_failed_or_unsafe_receipts()
-> Result<(), Box<dyn Error>> {
    for (failed, missing, unsafe_receipt) in [
        (None, Some("external-check"), None),
        (Some("validator-tool-install"), None, None),
        (None, Some("validator-tool-lock-admission"), None),
        (Some("validator-tool-lock-admission"), None, None),
        (
            None,
            Some("validator-tool-lock-admission"),
            Some("validator-tool-lock-admission"),
        ),
        (None, Some("root-preview"), Some("root-preview")),
    ] {
        let temporary = tempfile::tempdir()?;
        write_receipts(temporary.path(), failed, missing)?;
        if let Some(name) = unsafe_receipt {
            let logs = evidence_dir(temporary.path()).join("logs");
            #[cfg(unix)]
            std::os::unix::fs::symlink("../missing", logs.join(format!("{name}.exit")))?;
            #[cfg(not(unix))]
            return Err("unsafe receipt test requires symlink support".into());
        }
        let output = run_phase(
            temporary.path(),
            &["bundle-selected-qualification-evidence"],
        )?;
        assert_eq!(
            output.status.code(),
            Some(1),
            "failed={failed:?} missing={missing:?} unsafe={unsafe_receipt:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let evidence = evidence_dir(temporary.path());
        assert_eq!(
            fs::read_to_string(evidence.join("qualification-final-status.txt"))?,
            "qualification_checks_status=1\n"
        );
        assert_eq!(
            fs::read_to_string(evidence.join("logs/evidence-bundle.exit"))?,
            "0\n"
        );
        assert!(
            evidence
                .join("source-qualification-preserved.tar.gz")
                .is_file()
        );
    }
    Ok(())
}

fn write_receipts(
    root: &Path,
    failed: Option<&str>,
    missing: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let logs = evidence_dir(root).join("logs");
    fs::create_dir_all(&logs)?;
    fs::create_dir_all(evidence_dir(root).join("previews"))?;
    for name in REQUIRED_STATUSES {
        if Some(*name) == missing {
            continue;
        }
        let status = if Some(*name) == failed { "1\n" } else { "0\n" };
        fs::write(logs.join(format!("{name}.exit")), status)?;
    }
    if Some("root-preview") != missing {
        fs::write(
            logs.join("root-preview.exit"),
            "generate_statuses=0,0 identity_status=0 diff_statuses=0,0\n",
        )?;
    }
    Ok(())
}

fn evidence_dir(root: &Path) -> PathBuf {
    root.join("source-qualification-artifacts")
}

fn run_phase(root: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification.sh");
    Ok(Command::new("bash")
        .arg(script)
        .args(args)
        .env("RUNNER_TEMP", root)
        .output()?)
}
