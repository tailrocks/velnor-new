//! Generic Cargo binary-release workflow emission and ownership.

use std::fs;

use velnor_actions_orchestrator::{plan_text_checked, prepare, render_staged_tree};

use crate::impl_common::{TestResult, config_with_branch, make_repo, plan_for};

const CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.binary_release]\nenabled = true\nmanifest_path = \"Cargo.toml\"\npackage = \"demo\"\nbinary = \"demo\"\nsource_commit_env = \"REPO_SCAN_SOURCE_COMMIT\"\n";

#[test]
fn binary_release_is_generated_and_planned_only_when_enabled() -> TestResult {
    let enabled = make_repo(CONFIG)?;
    fs::write(enabled.path().join("src/main.rs"), "fn main() {}\n")?;
    let prep = prepare(enabled.path())?;
    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .files
        .iter()
        .find(|file| file.path == ".github/workflows/binary-release.yml")
        .expect("enabled binary release output");
    let yaml = &workflow.bytes;
    for expected in [
        "cron: 17 * * * *",
        "github.event_name == 'schedule'",
        "needs.verify-source.outputs.should_release == 'true'",
        "REPO_SCAN_SOURCE_COMMIT: ${{ env.SOURCE_SHA }}",
        "artifact-ids: ${{ needs.build-linux.outputs.artifact_id }}",
        "artifact-ids: ${{ needs.build-macos.outputs.artifact_id }}",
        "git merge-base --is-ancestor",
        "env -u GH_TOKEN",
        "contents: write",
    ] {
        assert!(yaml.contains(expected), "missing `{expected}`:\n{yaml}");
    }

    // Decode quoted run scalars before checking shell text, since YAML escapes
    // the script's quotes and newlines in the serialized workflow.
    let run_scripts = yaml
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("run: "))
        .map(|scalar| {
            if scalar.starts_with('"') {
                serde_json::from_str::<String>(scalar)
                    .expect("generated run script is a quoted YAML scalar")
            } else {
                scalar.to_owned()
            }
        })
        .collect::<Vec<_>>();
    assert!(
        !run_scripts.is_empty(),
        "workflow has generated run scripts"
    );
    let scripts = run_scripts.join("\n");
    for expected in [
        "refs/tags/$tag",
        "selected Cargo package or binary was not found",
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "sha256sum \"$linux_name\" \"$macos_name\" > SHA256SUMS",
        "source is no longer reachable from captured default branch",
    ] {
        assert!(
            scripts.contains(expected),
            "missing `{expected}` in generated run scripts:\n{scripts}"
        );
    }
    assert!(
        !yaml.contains("pull_request"),
        "release has no PR trigger or job"
    );
    assert!(!yaml.contains("push:"), "release has no tag push trigger");
    let publisher = &yaml[yaml.find("publish-release:").expect("publisher job")..];
    assert!(!publisher.contains("Check out exact source"));
    let plan = plan_for(&prep)?;
    assert!(plan.contains(".github/workflows/binary-release.yml"));

    let disabled = make_repo(config_with_branch())?;
    let disabled_prep = prepare(disabled.path())?;
    let disabled_tree = render_staged_tree(&disabled_prep)?;
    assert!(
        disabled_tree
            .files
            .iter()
            .all(|file| file.path != ".github/workflows/binary-release.yml")
    );
    assert!(!plan_text_checked(&disabled_prep)?.contains(".github/workflows/binary-release.yml"));
    Ok(())
}
