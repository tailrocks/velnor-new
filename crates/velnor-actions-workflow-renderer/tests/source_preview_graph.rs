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
    assert!(!workflow.contains("  source-preview-publication:"));
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
    for id in ["source-preview-build", "source-preview-qualify"] {
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
        assert!(preview.contains("runs-on: ubuntu-26.04"), "{id}");
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
    assert!(draft.contains("needs.source-preview-source.outputs.payload_artifact_id"));
    let admission = job(authority, "source-preview-source")?;
    assert!(!admission.contains("SDK_PREVIEW_VERSION"));
    assert!(admission.contains("source-preview-source-${{ github.run_attempt }}"));
    assert!(admission.contains("source-preview-qualified-payload-${{ github.run_attempt }}"));
    assert!(!admission.contains("/releases?per_page="));
    assert!(draft.contains("/releases?per_page=100&page="));
    assert!(draft.contains("source-preview-decision-${{ github.run_attempt }}"));
    assert!(draft.contains("steps.draft.outputs.source_artifact_id"));
    assert!(draft.contains("steps.draft.outputs.payload_artifact_id"));
    let attest = job(authority, "source-preview-attest")?;
    assert!(attest.contains("      contents: read"));
    assert!(!attest.contains("      contents: write"));
    assert!(attest.contains("needs.source-preview-draft.outputs.source_artifact_id"));
    assert!(attest.contains("needs.source-preview-draft.outputs.payload_artifact_id"));
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
    let qualifier = job(workflow, "source-preview-qualify")?;
    assert!(qualifier.contains("needs.source-preview-build.outputs.artifact_id"));
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
