// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::context_fixture::Fixture;
use serde_json::json;
use std::error::Error;

#[test]
fn rejects_graph_metadata_and_captured_attempt_changes_without_output() -> Result<(), Box<dyn Error>>
{
    let mut fixture = Fixture::new()?;
    let source = fixture
        .files
        .iter()
        .find(|(path, _)| path == ".github/workflows/product-release-images.yml")
        .ok_or("workflow")?
        .1
        .clone();
    for (old, new) in [
        (
            "needs.discover-producer.outputs.producer_artifact_id",
            "needs.discover-artifacts.outputs.producer_artifact_id",
        ),
        (
            "runner-image-publication-context-${{ github.run_attempt }}",
            "runner-image-publication-context-other",
        ),
        (
            "steps.assets.outputs.artifact-id",
            "steps.other.outputs.artifact-id",
        ),
    ] {
        let text = String::from_utf8(source.clone())?;
        assert!(text.contains(old));
        fixture.set_workflow(text.replacen(old, new, 1).into_bytes())?;
        let output = fixture.invoke()?;
        assert!(!output.status.success(), "accepted graph change {old}");
        assert_eq!(output.stdout, Vec::<u8>::new());
        fixture.set_workflow(source.clone())?;
    }
    Ok(())
}

#[test]
fn rejects_metadata_and_failed_jobs() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new()?;
    let original = fixture.state.clone();
    for (route, field, value) in [
        ("actions/artifacts/500", "expired", json!(true)),
        ("actions/artifacts/500", "size_in_bytes", json!(0)),
        (
            "actions/artifacts/500",
            "name",
            json!("runner-image-publication-context-1"),
        ),
        ("actions/artifacts/500", "digest", json!("sha256:bad")),
        ("actions/artifacts/400", "digest", json!("sha256:bad")),
        (
            "actions/runs/200/attempts/2",
            "event",
            json!("pull_request"),
        ),
        (
            "actions/runs/100/attempts/1",
            "path",
            json!(".github/workflows/other.yml"),
        ),
    ] {
        fixture.state = original.clone();
        fixture.state["context_api"][format!("repos/tailrocks/velnor-new/{route}")][field] = value;
        let output = fixture.invoke()?;
        assert!(!output.status.success(), "accepted {route}:{field}");
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
    for (route, job, field, value) in [
        (
            "actions/runs/200/attempts/2/jobs",
            0,
            "conclusion",
            json!("failure"),
        ),
        (
            "actions/runs/200/attempts/2/jobs",
            0,
            "run_attempt",
            json!(1),
        ),
        (
            "actions/runs/100/attempts/1/jobs",
            0,
            "conclusion",
            json!("failure"),
        ),
        (
            "actions/runs/100/attempts/1/jobs",
            0,
            "name",
            json!("Other"),
        ),
    ] {
        fixture.state = original.clone();
        fixture.state["context_api"][format!("repos/tailrocks/velnor-new/{route}")]["jobs"][job]
            [field] = value;
        let output = fixture.invoke()?;
        assert!(!output.status.success(), "accepted {route}:{field}");
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
    Ok(())
}

#[test]
fn rejects_ambiguous_artifacts_and_strict_json_violations() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new()?;
    let context = serde_json::to_string(&fixture.context)?;
    for raw in [
        "{".to_owned(),
        format!("{{\"schema\":\"duplicate\",{}", &context[1..]),
        context.replace("\"repository_id\":1390620900,", ""),
    ] {
        fixture.update_context_raw(raw.as_bytes())?;
        let output = fixture.invoke()?;
        assert!(!output.status.success());
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
    fixture.update_context()?;
    let route = "repos/tailrocks/velnor-new/actions/runs/200/artifacts";
    let artifact = fixture.state["context_api"][route]["artifacts"][0].clone();
    fixture.state["context_api"][route] = json!({"total_count":2,"artifacts":[artifact, {"id":501,"name":"runner-image-publication-context-2","expired":false,"size_in_bytes":12,"workflow_run":{"id":200,"head_sha":"a".repeat(40),"head_branch":"main"}}]});
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn rejects_cross_run_attempt_and_archive_inventory() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new()?;
    for extra in [false, true] {
        fixture.wrong_context_archive(extra)?;
        let output = fixture.invoke()?;
        assert!(!output.status.success());
        assert_eq!(output.stdout, Vec::<u8>::new());
        fixture.update_context()?;
    }
    fixture.symlink_context_archive()?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    fixture.update_context()?;
    fixture.update_context_raw(&vec![b'x'; 8193])?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    fixture.update_context()?;
    let original = fixture.state.clone();
    for (route, object, field, value) in [
        ("actions/artifacts/500", "workflow_run", "id", json!(201)),
        (
            "actions/artifacts/500",
            "workflow_run",
            "head_sha",
            json!("b".repeat(40)),
        ),
        (
            "actions/artifacts/500",
            "workflow_run",
            "head_branch",
            json!("other"),
        ),
        (
            "actions/runs/200/attempts/2",
            "head_repository",
            "id",
            json!(42),
        ),
        (
            "actions/runs/100/attempts/1",
            "head_repository",
            "id",
            json!(42),
        ),
    ] {
        fixture.state = original.clone();
        fixture.state["context_api"][format!("repos/tailrocks/velnor-new/{route}")][object]
            [field] = value;
        let output = fixture.invoke()?;
        assert!(
            !output.status.success(),
            "accepted {route}:{object}:{field}"
        );
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
    Ok(())
}

#[test]
fn rejects_changed_asset_bytes_before_returning_producer() -> Result<(), Box<dyn Error>> {
    for name in [
        "RUNNER_IMAGE_MANIFEST.json",
        "SHA256SUMS",
        "LICENSE",
        "NOTICE",
    ] {
        let fixture = Fixture::new()?;
        fixture.write_asset(name, b"tampered")?;
        let output = fixture.invoke()?;
        assert!(!output.status.success(), "accepted modified {name}");
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
    Ok(())
}

#[test]
fn rejects_duplicate_zip_members_with_matching_outer_digest() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new()?;
    fixture.duplicate_context_archive()?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn rejects_duplicate_publisher_members_with_matching_outer_digest() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new()?;
    fixture.duplicate_publisher_archive()?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}
