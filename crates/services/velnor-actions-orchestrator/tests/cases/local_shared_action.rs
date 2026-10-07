//! Local shared actions must pass pinned actionlint and zizmor.
use std::fs;

use velnor_actions_orchestrator::{
    ExecutionMode, GenerateOptions, generate, generate_dispatched, prepare,
};

use crate::support::{TestResult, git, make_repo};

fn both_config() -> &'static str {
    "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"both\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\"]\nplatform = \"linux/amd64\"\n"
}

fn committed_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(both_config())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "fixture"], root)?;
    Ok(repo)
}

#[test]
fn both_mode_generate_passes_actionlint_and_zizmor() -> TestResult {
    let repo = committed_repo()?;
    let prep = prepare(repo.path())?;
    let preview = tempfile::TempDir::new()?;
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.path().to_path_buf()),
        },
    )?;
    assert_eq!(
        report.validated_by,
        vec![
            "actionlint@1.7.12".to_owned(),
            "shellcheck@0.11.0".to_owned(),
            "zizmor@1.30.1".to_owned(),
        ]
    );
    let ci = fs::read_to_string(preview.path().join(".github/workflows/ci.yml"))?;
    let actionlint = fs::read_to_string(preview.path().join(".github/actionlint.yaml"))?;
    assert!(ci.contains("uses: ./.github/actions/"), "{ci}");
    assert!(!ci.contains("uses: $"), "{ci}");
    assert!(ci.len() <= 500_000, "ci.yml is {} bytes", ci.len());
    assert!(!actionlint.contains("paths:"), "{actionlint}");
    assert!(!actionlint.contains("shellcheck"), "{actionlint}");
    let action_dir = preview.path().join(".github/actions");
    let mut manifests = 0;
    for entry in fs::read_dir(action_dir)? {
        if entry?.path().join("action.yml").is_file() {
            manifests += 1;
        }
    }
    assert!(
        manifests > 0,
        "shared local action manifests were not emitted"
    );
    let hosted_dir = tempfile::TempDir::new()?;
    let hosted = generate_dispatched(
        &prep,
        &GenerateOptions {
            output_dir: Some(hosted_dir.path().to_path_buf()),
        },
        Some(ExecutionMode::Hosted),
    )?;
    assert!(
        hosted
            .validated_by
            .iter()
            .any(|spec| spec == "actionlint@1.7.12")
    );
    let hosted_lint = fs::read_to_string(hosted_dir.path().join(".github/actionlint.yaml"))?;
    let hosted_ci = fs::read_to_string(hosted_dir.path().join(".github/workflows/ci.yml"))?;
    assert!(!hosted_lint.contains("paths:"), "{hosted_lint}");
    assert!(!hosted_ci.contains("uses: $"), "{hosted_ci}");
    Ok(())
}
