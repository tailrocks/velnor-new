// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::{find_file, rendered};

#[test]
fn main_producer_tag_publisher_and_stable_calls_have_separate_authority_graphs()
-> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
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
    for id in [
        "source-preview-source",
        "source-preview-draft",
        "source-preview-attest",
        "source-preview-publish",
    ] {
        let tagged = job(workflow, id)?;
        assert!(tagged.contains("github.event_name == 'push'"), "{id}");
        assert!(
            tagged.contains("startsWith(github.ref, 'refs/tags/sdk-preview-')"),
            "{id}"
        );
        assert!(tagged.contains("runs-on: ubuntu-26.04"), "{id}");
        assert!(!tagged.contains("uses: actions/checkout@"), "{id}");
        assert!(!tagged.contains("mbx build"), "{id}");
    }
    let draft = job(workflow, "source-preview-draft")?;
    assert!(draft.contains("needs.source-preview-source.outputs.source_artifact_id"));
    assert!(draft.contains("needs.source-preview-source.outputs.payload_artifact_id"));
    for id in [
        "source-preview-draft",
        "source-preview-attest",
        "source-preview-publish",
    ] {
        let publication = job(workflow, id)?;
        assert!(
            publication.contains("    environment:\n      name: generator-source-preview\n"),
            "{id}"
        );
        assert!(
            !publication.contains("      name: generator-release\n"),
            "{id}"
        );
    }
    assert!(
        job(workflow, "publish-generator")?
            .contains("    environment:\n      name: generator-release\n")
    );
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
    let script = step_script(qualifier, "Qualify exact source preview bytes")?;
    assert!(script.contains(
        "check \"$GITHUB_WORKSPACE/source-preview/velnor-actions-x86_64-unknown-linux-gnu\""
    ));
    assert!(qualifier.contains("persist-credentials: \"false\""));
    assert!(script.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(!qualifier.contains(": write"));
    assert!(!qualifier.contains("id-token:"));
    assert!(!qualifier.contains("check-release"));
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
