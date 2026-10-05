//! Read-only pull-request generator-candidate workflow contract.

use std::error::Error;

use velnor_actions_workflow_renderer::RenderedTree;

pub(super) const GENERATOR_CANDIDATE: &str =
    include_str!("schema2_generator_candidate_snapshot.yml");
const MANIFEST_PRODUCER: &str =
    include_str!("../../../scripts/generator-release/create-release-manifest.sh");

pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn Error>> {
    let body = tree
        .get(".github/workflows/generator-candidate-qualification.yml")
        .ok_or("missing generated candidate qualification workflow")?;
    assert_eq!(body, super::marked(GENERATOR_CANDIDATE));
    assert_candidate(body)?;
    Ok(())
}

pub(super) fn assert_committed(root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let path = root.join(".github/workflows/generator-candidate-qualification.yml");
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(
        body,
        super::marked(GENERATOR_CANDIDATE),
        "{}",
        path.display()
    );
    assert_candidate(&body)?;
    Ok(())
}

fn assert_candidate(body: &str) -> Result<(), Box<dyn Error>> {
    assert_job_graph(body);
    assert_workflow_authority(body);
    assert_same_run_artifacts(body);
    assert_candidate_jobs(body)?;
    assert_required_fan_in(body)?;
    assert_native_runners(body)?;
    Ok(())
}

fn assert_job_graph(body: &str) {
    assert_eq!(
        super::job_ids(body),
        vec![
            "candidate-gate",
            "candidate-build-linux-x64",
            "candidate-build-macos-arm64",
            "candidate-build-macos-x64",
            "candidate-prepare-manifest",
            "candidate-qualify-linux-x64",
            "candidate-qualify-macos-arm64",
            "candidate-qualify-macos-x64",
            "candidate-qualification-required",
        ]
    );
}

fn assert_workflow_authority(body: &str) {
    assert!(body.contains("name: Generator candidate qualification\n"));
    assert!(body.contains("pull_request:\n"));
    assert!(body.contains("branches:\n      - main\n"));
    assert!(!body.contains("workflow_dispatch:"), "{body}");
    assert!(body.contains("permissions:\n  actions: read\n  contents: read\n"));
    for forbidden in [
        "actions: write",
        "contents: write",
        "id-token:",
        "attestations:",
        "artifact-metadata:",
        "secrets.",
        "GH_TOKEN",
        "environment:",
        "gh release create",
        "git tag",
    ] {
        assert!(
            !body.contains(forbidden),
            "forbidden candidate capability {forbidden}"
        );
    }
}

fn assert_same_run_artifacts(body: &str) {
    assert!(body.contains("github.event.pull_request.base.ref == 'main'"));
    assert!(body.contains("GITHUB_WORKFLOW_REF"));
    assert!(body.contains("GITHUB_WORKFLOW_SHA"));
    assert!(body.contains("git rev-parse HEAD"));
    assert!(body.contains("scripts/generator-release/create-release-manifest.sh"));
    assert!(MANIFEST_PRODUCER.contains("--arg commit \"$GITHUB_SHA\""));
    assert!(body.contains("outputs.asset_id"));
    assert!(body.contains("outputs.manifest_id"));
    assert!(body.contains("outputs.manifest_sha256"));
    assert!(body.contains("github.run_id"));
    assert!(body.contains("github.run_attempt"));
    assert!(body.contains("outputs.source_sha"));
    for prefix in [
        "generator-pr-candidate-linux-x64-assets",
        "generator-pr-candidate-macos-arm64-assets",
        "generator-pr-candidate-macos-x64-assets",
        "generator-pr-candidate-release-manifest",
    ] {
        assert!(
            body.contains(&format!(
                "{prefix}-run-${{{{ github.run_id }}}}-attempt-${{{{ github.run_attempt }}}}"
            )),
            "{body}"
        );
    }
    assert!(body.contains("artifact-ids:"));
    assert!(!body.contains("artifact-ids: generator-pr-candidate"));
    assert!(body.contains("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"));
    assert!(body.contains("actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c"));
    assert!(body.contains("jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5"));
    assert!(body.contains("version: 2026.9.18"));
}

fn assert_candidate_jobs(body: &str) -> Result<(), Box<dyn Error>> {
    for job in [
        "candidate-build-linux-x64",
        "candidate-build-macos-arm64",
        "candidate-build-macos-x64",
        "candidate-prepare-manifest",
        "candidate-qualify-linux-x64",
        "candidate-qualify-macos-arm64",
        "candidate-qualify-macos-x64",
    ] {
        let body = super::job_body(body, job)?;
        assert!(body.contains("candidate-context.outputs.run_id"), "{body}");
        assert!(
            body.contains("candidate-context.outputs.run_attempt"),
            "{body}"
        );
        assert!(
            body.contains("candidate-context.outputs.source_sha"),
            "{body}"
        );
        assert!(
            body.contains("outputs.run_attempt == github.run_attempt"),
            "{body}"
        );
        assert!(body.contains("outputs.run_id == github.run_id"), "{body}");
        assert!(body.contains("outputs.source_sha == github.sha"), "{body}");
    }
    assert_qualifier_jobs(body)?;
    Ok(())
}

fn assert_qualifier_jobs(workflow: &str) -> Result<(), Box<dyn Error>> {
    for id in [
        "candidate-qualify-linux-x64",
        "candidate-qualify-macos-arm64",
        "candidate-qualify-macos-x64",
    ] {
        let body = super::job_body(workflow, id)?;
        assert!(
            body.contains("permissions:\n      actions: read\n      contents: read\n"),
            "candidate qualifiers need read-only checkout and artifact permissions: {body}"
        );
        assert!(body.contains("needs.candidate-gate.result == 'success'"));
        assert!(body.contains("needs.candidate-prepare-manifest.result == 'success'"));
        let source_bind = body
            .find("Bind workflow authority to merge candidate source")
            .ok_or("candidate source binding step is missing")?;
        let install = body
            .find("Install catalog-pinned qualification tools")
            .ok_or("pinned qualification tool install is missing")?;
        let download = body
            .find("Download exact pull-request candidate artifact")
            .ok_or("candidate artifact download is missing")?;
        assert!(source_bind < install && install < download, "{body}");
        assert!(body.contains("mise --no-config --no-env --no-hooks install"));
        assert!(body.contains("Bind candidate bytes to same-run manifest"));
        assert!(body.contains("check-release"));
        assert!(body.contains("chmod +x \\\"$candidate\\\""));
    }
    Ok(())
}

fn assert_required_fan_in(body: &str) -> Result<(), Box<dyn Error>> {
    let required = super::job_body(body, "candidate-qualification-required")?;
    assert!(required.contains("if: always()"), "{required}");
    assert!(required.contains("permissions: {}"), "{required}");
    for job in [
        "candidate-gate",
        "candidate-build-linux-x64",
        "candidate-build-macos-arm64",
        "candidate-build-macos-x64",
        "candidate-prepare-manifest",
        "candidate-qualify-linux-x64",
        "candidate-qualify-macos-arm64",
        "candidate-qualify-macos-x64",
    ] {
        assert!(required.contains(&format!("- {job}\n")), "{required}");
        let key = job.replace('-', "_").to_ascii_uppercase();
        for field in ["RESULT", "RUN_ID", "RUN_ATTEMPT", "SOURCE_SHA"] {
            assert!(
                required.contains(&format!("VELNOR_NEEDS_{key}_{field}")),
                "{required}"
            );
        }
    }
    assert!(required.contains("github.run_id"), "{required}");
    assert!(required.contains("github.run_attempt"), "{required}");
    assert!(required.contains("github.sha"), "{required}");
    assert!(required.contains("= \\\"success\\\""), "{required}");
    Ok(())
}

fn assert_native_runners(body: &str) -> Result<(), Box<dyn Error>> {
    for (job, runner) in [
        ("candidate-build-linux-x64", "runs-on: ubuntu-26.04\n"),
        ("candidate-build-macos-arm64", "runs-on: macos-15\n"),
        ("candidate-build-macos-x64", "runs-on: macos-15-intel\n"),
        ("candidate-qualify-linux-x64", "runs-on: ubuntu-26.04\n"),
        ("candidate-qualify-macos-arm64", "runs-on: macos-15\n"),
        ("candidate-qualify-macos-x64", "runs-on: macos-15-intel\n"),
    ] {
        assert!(super::job_body(body, job)?.contains(runner), "{job}");
    }
    Ok(())
}
