//! Full native generation validates both channels' tap continuations.
use super::{tap_enabled_config, tap_enabled_repo};
use crate::impl_common::TestResult;
use velnor_actions_contract::package_release::PackageReleaseChannel;
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
            "[distribution.homebrew_tap.review_completion]",
            "[distribution.homebrew_tap.destinations.stable]\nformula = \"Formula/demo.rb\"\n[distribution.homebrew_tap.review_completion]",
        )
        .replace(
            "conflicts = [] }]",
            "conflicts = [] }, { channel = \"stable\", formula = \"demo\", formula_class = \"Demo\", destination = \"Formula/demo.rb\", conflicts = [] }]",
        );
    assert!(config.contains("[distribution.homebrew_tap.destinations.stable]"));
    assert!(config.contains("channel = \"stable\", formula = \"demo\""));
    let repo = tap_enabled_repo(&config)?;
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
    for (workflow, channel) in [
        ("preview-tap-reconcile.yml", PackageReleaseChannel::Preview),
        ("stable-tap-reconcile.yml", PackageReleaseChannel::Stable),
    ] {
        let yaml = std::fs::read_to_string(generated.join(workflow))?;
        assert!(
            yaml.contains(&format!("name: {}", channel.tap_reconcile_workflow_name())),
            "{workflow}: {yaml}"
        );
        assert!(!yaml.contains("inputs."), "{workflow}: {yaml}");
        assert!(!yaml.contains("env: {}"), "{workflow}: {yaml}");
        assert!(
            yaml.contains("VELNOR_RELEASE_ORIGINAL_RUN_ID:"),
            "{workflow}"
        );
    }
    let stable = std::fs::read_to_string(generated.join("binary-release.yml"))?;
    assert!(
        stable.contains(&format!(
            "name: {}",
            PackageReleaseChannel::Stable.workflow_name()
        )),
        "{stable}"
    );
    assert!(stable.contains("VELNOR_STABLE_OPERATION: ${{ inputs.operation }}"));
    assert!(stable.contains("VELNOR_STABLE_VERSION_PACKAGE: demo"));
    let preview = std::fs::read_to_string(generated.join("binary-preview.yml"))?;
    assert!(
        preview.contains(&format!(
            "name: {}",
            PackageReleaseChannel::Preview.workflow_name()
        )),
        "{preview}"
    );
    assert!(preview.contains("publish-package-release-v1"));
    assert!(preview.contains("VELNOR_RELEASE_PUBLICATION_TIMEOUT_MINUTES: \"15\""));
    Ok(())
}
