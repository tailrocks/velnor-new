//! G2 end to end: install lockfile audit blocks holes, advises gaps.
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::StepKind;
use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, make_repo, plan_for};

/// Consumer fixture install set: plan + crate + required Prepare tools.
const PINS: [(&str, &str); 5] = [
    ("rust", "1.98.1"),
    ("actionlint", "1.7.12"),
    ("shellcheck", "0.11.0"),
    ("zizmor", "1.30.1"),
    ("gh", "2.102.0"),
];

/// v3 lock entry; `platforms` selects checksummed platforms (`rust` takes none).
fn entry(tool: &str, version: &str, platforms: &[&str], seed: u8) -> String {
    let mut out = format!("[[tools.{tool}]]\nversion = \"{version}\"\n");
    for platform in platforms {
        let hex = format!("{seed:02x}").repeat(32);
        let block = format!(
            "\n[tools.{tool}.\"platforms.{platform}\"]\nchecksum = \"sha256:{hex}\"\nurl = \"https://example.invalid/{tool}/{platform}\"\n"
        );
        out.push_str(&block);
    }
    out
}

/// Full-coverage lock: every pin matches, checksums where backends emit them.
fn full_lock() -> String {
    let mut lock = String::from("lockfile_version = 3\n\n");
    for (tool, pin) in PINS {
        let (platforms, seed): (&[&str], u8) = match tool {
            "rust" => (&[], 0x00),
            "actionlint" => (&["linux-x64", "macos-arm64"], 0x0a),
            "shellcheck" => (&["linux-x64", "macos-arm64"], 0x0b),
            "zizmor" => (&["linux-x64", "macos-arm64"], 0x0c),
            _ => (&["linux-x64", "macos-arm64"], 0x0d),
        };
        lock.push_str(&entry(tool, pin, platforms, seed));
    }
    lock
}

fn repo_with_lock(lock: Option<&str>) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    if let Some(text) = lock {
        fs::write(repo.path().join("mise.lock"), text)?;
    }
    Ok(repo)
}

fn preview_generate(repo: &TempDir) -> Result<String, Box<dyn std::error::Error>> {
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview),
        },
    )?;
    plan_for(&prep)
}

#[test]
fn missing_lock_advises_and_generates() -> TestResult {
    let repo = repo_with_lock(None)?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.lock_audit_blocking, [] as [std::string::String; 0]);
    let summary = prep
        .discovery
        .recommendations
        .iter()
        .find(|line| line.contains("tool_install_unverified"))
        .ok_or("missing-lock summary")?;
    assert!(summary.contains("no mise.lock"), "{summary}");
    for tool in [
        "rust@1.98.1",
        "actionlint@1.7.12",
        "shellcheck@0.11.0",
        "zizmor@1.30.1",
        "gh@2.102.0",
    ] {
        assert!(summary.contains(tool), "{summary}");
    }
    let plan = preview_generate(&repo)?;
    assert!(plan.contains("tool_install_unverified"), "{plan}");
    Ok(())
}

#[test]
fn full_lock_leaves_only_rust_unverifiable() -> TestResult {
    let repo = repo_with_lock(Some(&full_lock()))?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.lock_audit_blocking, [] as [std::string::String; 0]);
    let summary = prep
        .discovery
        .recommendations
        .iter()
        .find(|line| line.contains("tool_install_unverified"))
        .ok_or("rust-only summary")?;
    assert!(summary.contains("rust@1.98.1(no_checksums)"), "{summary}");
    for covered in ["actionlint", "shellcheck", "zizmor", "gh@"] {
        assert!(!summary.contains(covered), "{summary}");
    }
    preview_generate(&repo)?;
    Ok(())
}

#[test]
fn platform_hole_blocks_generate_with_remediation() -> TestResult {
    let hex = "0a".repeat(32);
    let holed = full_lock().replace(
        &format!(
            "[tools.actionlint.\"platforms.linux-x64\"]\nchecksum = \"sha256:{hex}\"\nurl = \"https://example.invalid/actionlint/linux-x64\"\n"
        ),
        "",
    );
    assert!(!holed.contains("actionlint.\"platforms.linux-x64\""));
    let repo = repo_with_lock(Some(&holed))?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.lock_audit_blocking.len(), 1);
    let diagnostic = &prep.lock_audit_blocking[0];
    assert!(diagnostic.contains("lock_missing_platform"), "{diagnostic}");
    assert!(diagnostic.contains("actionlint@1.7.12"), "{diagnostic}");
    assert!(
        diagnostic.contains("run 'mise lock' and commit the resulting checksums"),
        "{diagnostic}"
    );
    assert!(
        diagnostic
            .contains("if a backend publishes no checksums, install on linux-x64 and commit those"),
        "{diagnostic}"
    );
    let plan = plan_for(&prep)?;
    assert!(plan.contains("lock_missing_platform"), "{plan}");
    let parent = TempDir::new()?;
    let err = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(parent.path().join("preview")),
        },
    )
    .expect_err("platform hole must block");
    assert!(err.to_string().contains("lock_audit_blocked"), "{err}");
    assert!(err.to_string().contains("lock_missing_platform"), "{err}");
    Ok(())
}

#[test]
fn corrupt_checksum_blocks_generate() -> TestResult {
    let hex = "0a".repeat(32);
    let tampered = full_lock().replace(&format!("sha256:{hex}"), "not-a-checksum");
    let repo = repo_with_lock(Some(&tampered))?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.lock_audit_blocking.len(), 1);
    assert!(
        prep.lock_audit_blocking[0].contains("lock_corrupt_checksum"),
        "{:?}",
        prep.lock_audit_blocking
    );
    let parent = TempDir::new()?;
    let err = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(parent.path().join("preview")),
        },
    )
    .expect_err("corrupt checksum must block");
    assert!(err.to_string().contains("lock_corrupt_checksum"), "{err}");
    Ok(())
}

/// Hostile `mise.toml` is ignored by isolated installs: every emitted
/// install argv carries `--no-config`, so repo config (`postinstall`
/// hooks, custom `[plugins]`) can never execute during installs and
/// `generate` succeeds.
#[test]
fn hostile_config_ignored_by_isolated_installs() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(
        repo.path().join("mise.toml"),
        "[tools.rust]\nversion = \"1.98.1\"\npostinstall = \"touch pwned\"\n\n[plugins.evil]\nurl = \"https://example.invalid/evil\"\n",
    )?;
    let prep = prepare(repo.path())?;
    assert!(
        prep.lock_audit_blocking.is_empty(),
        "{:?}",
        prep.lock_audit_blocking
    );
    let parent = TempDir::new()?;
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(parent.path().join("preview")),
        },
    )?;
    let mut installs: Vec<String> = Vec::new();
    for job in prep.workflow.ir.jobs.values() {
        for step in &job.steps {
            if step.name != PREPARE_PINNED_TOOLS_STEP {
                continue;
            }
            let StepKind::Shell { run, .. } = &step.kind else {
                continue;
            };
            installs.push(run.join(" "));
        }
    }
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &prep.workflow.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
    )
    .map_err(|err| format!("render: {err}"))?;
    for line in yaml.lines() {
        if line.contains("mise ") && line.contains(" install ") {
            installs.push(line.trim().to_owned());
        }
    }
    assert!(!installs.is_empty(), "fixture must emit installs");
    for argv in &installs {
        assert!(
            argv.contains("--no-config"),
            "install must be config-isolated: {argv}"
        );
    }
    Ok(())
}

#[test]
fn drifted_lock_advises_without_blocking() -> TestResult {
    let drifted = full_lock().replace("version = \"1.98.1\"", "version = \"1.97.0\"");
    let repo = repo_with_lock(Some(&drifted))?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.lock_audit_blocking, [] as [std::string::String; 0]);
    let summary = prep
        .discovery
        .recommendations
        .iter()
        .find(|line| line.contains("tool_install_unverified"))
        .ok_or("drift summary")?;
    assert!(summary.contains("version_drift"), "{summary}");
    preview_generate(&repo)?;
    Ok(())
}
