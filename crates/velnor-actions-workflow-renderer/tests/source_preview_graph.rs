// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::{find_file, rendered};

#[path = "../src/schema2_preview_permissions.rs"]
mod preview_permissions;

#[test]
fn preview_caller_delegates_only_required_permissions() {
    use preview_permissions::{PreviewPermissionRole, preview_permissions};
    assert_eq!(
        preview_permissions(PreviewPermissionRole::Caller),
        &[
            ("actions", "read"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "write"),
            ("id-token", "write"),
        ]
    );
    assert_eq!(
        preview_permissions(PreviewPermissionRole::Attest),
        &[
            ("actions", "read"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "read"),
            ("id-token", "write"),
        ]
    );
    assert_eq!(
        preview_permissions(PreviewPermissionRole::Publish),
        &[("actions", "read"), ("contents", "write"),]
    );
}

#[test]
fn main_producer_tag_publisher_and_stable_calls_have_separate_authority_graphs()
-> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
    let authority = find_file(&files, ".github/workflows/source-preview-publication.yml")?;
    assert_caller_bootstrap(workflow)?;
    assert_reusable_authority(workflow, authority)?;
    Ok(())
}

fn assert_caller_bootstrap(workflow: &str) -> Result<(), Box<dyn Error>> {
    let callers: Vec<_> = workflow
        .match_indices("  source-preview-publication:")
        .collect();
    assert_eq!(callers.len(), 1);
    let caller = job(workflow, "source-preview-publication")?;
    assert!(caller.contains("if: ${{ github.event_name == 'push' && github.repository == 'tailrocks/velnor-new' && startsWith(github.ref, 'refs/tags/sdk-preview-') }}"));
    assert!(caller.contains("uses: tailrocks/velnor-new/.github/workflows/source-preview-publication.yml@72701af69f6f9b693e639814fcf479409da3295c"));
    assert!(caller.contains("    permissions:\n      actions: read\n      artifact-metadata: write\n      attestations: write\n      contents: write\n      id-token: write\n"));
    assert!(!caller.contains("    needs:"));
    assert!(!workflow.contains("  workflow_dispatch:"));
    assert!(workflow.contains("  push:"));
    assert!(workflow.contains("sdk-preview-*"));
    assert!(workflow.contains("  workflow_call:"));
    let caller = job(workflow, "verify-release-caller")?;
    assert!(caller.contains("github.event_name != 'push'"));
    assert!(caller.contains("github.workflow_ref != 'tailrocks/velnor-new/.github/workflows/product-release-generator.yml@refs/heads/main'"));
    for id in [
        "build-linux",
        "build-macos",
        "build-macos-intel",
        "publish-generator",
    ] {
        let stable = job(workflow, id)?;
        assert!(stable.contains("inputs.release_action == 'build'"), "{id}");
        assert!(stable.contains("verify-release-caller"), "{id}");
    }
    for id in [
        "source-preview-build-linux",
        "source-preview-qualify-linux",
        "source-preview-build-darwin",
        "source-preview-qualify-darwin",
    ] {
        let preview = job(workflow, id)?;
        assert!(preview.contains("github.event_name == 'push'"), "{id}");
        assert!(
            preview.contains("github.repository == 'tailrocks/velnor-new'"),
            "{id}"
        );
        assert!(
            preview.contains("product-release-generator.yml@refs/heads/main'"),
            "{id}"
        );
        assert!(preview.contains("github.ref == 'refs/heads/main'"), "{id}");
        assert!(!preview.contains(": write"), "{id}");
        let runner = if id.ends_with("darwin") {
            "macos-15"
        } else {
            "ubuntu-26.04"
        };
        assert!(preview.contains(&format!("runs-on: {runner}")), "{id}");
        assert!(!preview.contains("verify-release-caller"), "{id}");
    }
    assert!(
        job(workflow, "publish-generator")?
            .contains("    environment:\n      name: generator-release\n")
    );
    Ok(())
}

fn assert_reusable_authority(workflow: &str, authority: &str) -> Result<(), Box<dyn Error>> {
    assert!(authority.contains("  workflow_call:"));
    for (id, permissions) in [
        (
            "source-preview-source",
            "      contents: read\n      actions: read\n",
        ),
        (
            "source-preview-draft",
            "      actions: read\n      contents: write\n",
        ),
        (
            "source-preview-attest",
            "      actions: read\n      artifact-metadata: write\n      attestations: write\n      contents: read\n      id-token: write\n",
        ),
        (
            "source-preview-publish",
            "      actions: read\n      contents: write\n",
        ),
    ] {
        let expected = format!("    permissions:\n{permissions}");
        assert!(job(authority, id)?.contains(&expected), "{id}");
    }
    assert!(job(workflow, "attest-linux")?.contains("    permissions:\n      actions: write\n"));
    assert!(
        job(workflow, "publish-generator")?
            .contains("    permissions:\n      actions: write\n      contents: write\n")
    );
    for id in [
        "source-preview-source",
        "source-preview-draft",
        "source-preview-attest",
        "source-preview-publish",
    ] {
        assert!(job(workflow, id).is_err(), "candidate caller contains {id}");
        let tagged = job(authority, id)?;
        assert!(tagged.contains("github.event_name == 'push'"), "{id}");
        assert!(
            tagged.contains("startsWith(github.ref, 'refs/tags/sdk-preview-')"),
            "{id}"
        );
        assert!(tagged.contains("runs-on: ubuntu-26.04"), "{id}");
        assert!(!tagged.contains("uses: actions/checkout@"), "{id}");
        assert!(!tagged.contains("mbx build"), "{id}");
        assert!(tagged.contains("${{ job.workflow_repository }}"));
        assert!(tagged.contains("${{ job.workflow_file_path }}"));
        assert!(tagged.contains("${{ job.workflow_ref }}"));
        assert!(tagged.contains("${{ job.workflow_sha }}"));
    }
    for id in [
        "source-preview-draft",
        "source-preview-attest",
        "source-preview-publish",
    ] {
        let publication = job(authority, id)?;
        assert!(
            publication.contains("    environment:\n      name: generator-source-preview\n"),
            "{id}"
        );
        assert!(
            !publication.contains("      name: generator-release\n"),
            "{id}"
        );
    }
    assert_publication_artifacts(authority)?;
    Ok(())
}

fn assert_publication_artifacts(authority: &str) -> Result<(), Box<dyn Error>> {
    let draft = job(authority, "source-preview-draft")?;
    assert!(draft.contains("needs.source-preview-source.outputs.source_artifact_id"));
    assert!(draft.contains("needs.source-preview-source.outputs.linux_payload_artifact_id"));
    assert!(draft.contains("needs.source-preview-source.outputs.darwin_payload_artifact_id"));
    let admission = job(authority, "source-preview-source")?;
    assert!(!admission.contains("SDK_PREVIEW_VERSION"));
    assert!(admission.contains("source-preview-source-${{ github.run_attempt }}"));
    assert!(admission.contains("source-preview-qualified-payload-linux-${{ github.run_attempt }}"));
    assert!(
        admission.contains("source-preview-qualified-payload-darwin-${{ github.run_attempt }}")
    );
    assert!(!materialized_script(admission)?.contains("/releases?per_page="));
    assert!(materialized_script(draft)?.contains("/releases?per_page=100&page="));
    assert!(draft.contains("source-preview-decision-${{ github.run_attempt }}"));
    assert!(draft.contains("steps.draft.outputs.source_artifact_id"));
    assert!(draft.contains("steps.draft.outputs.linux_payload_artifact_id"));
    assert!(draft.contains("steps.draft.outputs.darwin_payload_artifact_id"));
    let attest = job(authority, "source-preview-attest")?;
    assert!(attest.contains("      contents: read"));
    assert!(!attest.contains("      contents: write"));
    assert!(attest.contains("needs.source-preview-draft.outputs.source_artifact_id"));
    assert!(attest.contains("needs.source-preview-draft.outputs.linux_payload_artifact_id"));
    assert!(attest.contains("needs.source-preview-draft.outputs.darwin_payload_artifact_id"));
    assert!(attest.contains("steps.prepare.outputs.sign == 'true'"));
    assert!(attest.contains("source-preview-signed-${{ github.run_attempt }}"));
    assert!(attest.contains("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"));
    assert!(attest.contains("steps.prepare.outputs.pair_id || steps.upload.outputs.artifact-id"));
    let publish = job(authority, "source-preview-publish")?;
    assert!(publish.contains("needs.source-preview-attest.outputs.pair_id"));
    assert!(publish.contains("needs.source-preview-attest.outputs.pair_origin == 'artifact'"));
    Ok(())
}

#[test]
fn preview_qualifier_executes_exact_downloaded_payload_without_publication_authority()
-> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
    let qualifier = job(workflow, "source-preview-qualify-linux")?;
    assert!(qualifier.contains("needs.source-preview-build-linux.outputs.artifact_id"));
    assert!(qualifier.contains("source-preview/velnor-actions-x86_64-unknown-linux-gnu"));
    assert!(qualifier.contains("capture-opentofu-goldens.sh"));
    assert!(qualifier.contains("Restore Cargo sources"));
    assert!(qualifier.contains("actions/cache/restore@"));
    assert!(!qualifier.contains("actions/cache/save@"));
    assert!(qualifier.contains("runner.temp }}/velnor/cargo/registry/index"));
    assert!(qualifier.contains("runner.temp }}/velnor/cargo/registry/cache"));
    let script = step_script(qualifier, "Qualify exact source preview bytes")?;
    assert!(script.contains(
        "check \"$GITHUB_WORKSPACE/source-preview/velnor-actions-x86_64-unknown-linux-gnu\""
    ));
    assert!(qualifier.contains("persist-credentials: \"false\""));
    assert!(script.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(!qualifier.contains(": write"));
    assert!(!qualifier.contains("id-token:"));
    assert!(!qualifier.contains("check-release"));
    let ordered = [
        "test \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"",
        "CARGO_HOME=",
        "export CARGO_HOME",
        "prefetch-locked-sources-for-qualification-workspaces",
        "cargo metadata --locked --offline --all-features",
        "build-owned-archive-guard.sh --source-cache",
        "--version",
        "capture-opentofu-goldens.sh",
    ];
    let positions = ordered
        .iter()
        .map(|needle| script.find(needle).ok_or("missing qualification step"))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    let mac = job(workflow, "source-preview-qualify-darwin")?;
    assert!(mac.contains("runs-on: macos-15"));
    assert!(mac.contains("needs.source-preview-build-darwin.outputs.artifact_id"));
    assert!(!mac.contains(": write"));
    assert!(!mac.contains("id-token:"));
    let mac_script = step_script(mac, "Qualify exact source preview bytes")?;
    assert!(mac_script.contains("source-preview/velnor-actions-aarch64-apple-darwin"));
    assert!(mac_script.contains("capture-opentofu-goldens.sh"));
    assert!(mac_script.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    let mac_positions = ordered
        .iter()
        .map(|needle| {
            mac_script
                .find(needle)
                .ok_or("missing Darwin qualification step")
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert!(mac_positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(mac_script.contains("shasum -a 256"));
    assert!(!mac_script.contains("sha256sum"));
    assert!(!mac.contains("id: sources-restore"));
    assert!(!mac.contains("sha256sum"));
    Ok(())
}

fn step_script(job: &str, name: &str) -> Result<String, Box<dyn Error>> {
    let marker = format!("- name: {name}");
    let mut lines = job.lines().skip_while(|line| line.trim() != marker);
    lines.next().ok_or("missing step")?;
    let encoded = lines
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or("missing step script")?;
    Ok(serde_json::from_str(encoded)?)
}

fn job<'a>(workflow: &'a str, id: &str) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("\n  {id}:\n");
    let offset = workflow
        .find(&marker)
        .ok_or_else(|| format!("missing {id}"))?;
    let body = &workflow[offset + marker.len()..];
    let end = body
        .lines()
        .scan(0, |position, line| {
            let start = *position;
            *position += line.len() + 1;
            Some((start, line))
        })
        .find_map(|(start, line)| {
            (line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
                .then_some(start)
        })
        .unwrap_or(body.len());
    Ok(&body[..end])
}

fn materialized_script(job: &str) -> Result<String, Box<dyn Error>> {
    let runs = crate::bounded_test_support::materializer_runs(job)?;
    crate::bounded_test_support::materialized_source(&runs)
}

#[test]
fn preview_publication_preserves_fixed_source_and_executor_authority() -> Result<(), Box<dyn Error>>
{
    let files = rendered()?;
    let authority = find_file(&files, ".github/workflows/source-preview-publication.yml")?;
    for (id, step, expected_id) in [
        (
            "source-preview-source",
            "Admit annotated SDK preview tag",
            None,
        ),
        (
            "source-preview-draft",
            "Create exact preview draft",
            Some("draft"),
        ),
        (
            "source-preview-attest",
            "Select or finalize signed candidate",
            Some("prepare"),
        ),
        ("source-preview-publish", "Verify and publish preview", None),
    ] {
        let section = job(authority, id)?;
        let body = materialized_script(section)?;
        assert!(body.len() > 21_000, "{id}");
        let expected = if id == "source-preview-source" {
            include_str!("../src/schema2_generator_source_preview_source.py")
        } else {
            include_str!("../src/schema2_generator_source_preview_publication.py")
        };
        assert!(body.contains(expected), "{id}");
        let (materializers, executor) = section
            .split_once(&format!("- name: {step}"))
            .ok_or("missing executor")?;
        let materializers = materializers
            .rsplit("- name: Materialize fixed script")
            .next()
            .ok_or("missing materializer")?;
        assert!(!materializers.contains("GH_TOKEN"), "{id}");
        for line in materializers
            .lines()
            .filter_map(|line| line.trim().strip_prefix("run: "))
        {
            let run: String = serde_json::from_str(line)?;
            assert!(run.chars().count() <= 21_000, "{id}");
        }
        let executor = executor
            .split("\n      - name:")
            .next()
            .ok_or("missing executor fields")?;
        assert!(executor.contains("GH_TOKEN: ${{ github.token }}"), "{id}");
        assert!(executor.contains("shell: bash"), "{id}");
        if let Some(step_id) = expected_id {
            assert!(executor.contains(&format!("id: {step_id}")), "{id}");
        }
        let run = step_script(section, step)?;
        assert!(run.chars().count() <= 21_000, "{id}");
        assert!(run.contains("-e -o pipefail"), "{id}");
    }
    Ok(())
}

#[path = "source_preview_publication_bounds.rs"]
mod publication_bounds;

#[path = "source_preview_runtime_graph.rs"]
mod runtime_graph;
