//! Full native generation validates both channels' tap continuations.
use super::{binary_repo, tap_enabled_config};
use crate::impl_common::{TestResult, git};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

#[test]
fn both_tap_reconciliation_workflows_pass_pinned_jactionlint() -> TestResult {
    let config = tap_enabled_config(true)
        .replace(
            "[stacks.rust.binary_release]\n",
            "[stacks.rust.binary_release]\npublication_timeout_minutes = 15\n",
        )
        .replace(
            "inputs = [\"mise.toml\", \"Formula/demo-preview.rb\", \"scripts/package-update.sh\"]",
            "inputs = [\"mise.toml\", \"Formula/demo-preview.rb\", \"Formula/demo.rb\", \"scripts/package-update.sh\"]",
        )
        .replace(
            "[distribution.homebrew_tap.updater]",
            "[distribution.homebrew_tap.destinations.stable]\nformula = \"Formula/demo.rb\"\n[distribution.homebrew_tap.updater]",
        );
    let repo = binary_repo(&config)?;
    git(&["add", "mise.toml"], repo.path())?;
    let prepared = prepare(repo.path())?;
    let output = tempfile::tempdir()?;
    let report = generate(
        &prepared,
        &GenerateOptions {
            output_dir: Some(output.path().join("generated")),
        },
    )?;
    assert!(
        report
            .validated_by
            .iter()
            .any(|tool| tool.starts_with("jactionlint@")),
        "native generation must run the pinned workflow validator"
    );

    let generated = output.path().join("generated/.github/workflows");
    for workflow in ["preview-tap-reconcile.yml", "stable-tap-reconcile.yml"] {
        let yaml = std::fs::read_to_string(generated.join(workflow))?;
        assert!(!yaml.contains("inputs."), "{workflow}: {yaml}");
        assert!(!yaml.contains("env: {}"), "{workflow}: {yaml}");
        assert!(
            yaml.contains("VELNOR_RELEASE_ORIGINAL_RUN_ID:"),
            "{workflow}"
        );
    }
    let stable = std::fs::read_to_string(generated.join("binary-release.yml"))?;
    assert!(stable.contains("VELNOR_STABLE_OPERATION: ${{ inputs.operation }}"));
    assert!(stable.contains("VELNOR_STABLE_VERSION_PACKAGE: demo"));
    let preview = std::fs::read_to_string(generated.join("binary-preview.yml"))?;
    assert!(preview.contains("publish-package-release-v1"));
    assert!(preview.contains("VELNOR_RELEASE_PUBLICATION_TIMEOUT_MINUTES: \"15\""));
    Ok(())
}
