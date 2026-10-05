use super::{
    LINUX_TARGET, MACOS_X86_64_TARGET, MANIFEST_NAME, RELEASE_VERSION, TARGET_FIXTURES,
    assert_failure, assert_success, asset_record, asset_records, asset_url, output_text,
    release_json,
};
use std::error::Error;
use std::fs;

const MANIFEST_CHECKSUM_NAME: &str = "velnor-actions-release-manifest.json.sha256";
const ACCEPTANCE_NAME: &str = "velnor-actions-release-acceptance.json";

#[path = "schema2_generator_release_publisher_fixture.rs"]
mod fixture;
use self::fixture::PublisherFixture;

#[test]
fn publisher_emits_acceptance_only_after_complete_immutable_verification()
-> Result<(), Box<dyn Error>> {
    let fixture = PublisherFixture::new("publish-valid")?;
    let records = asset_records(RELEASE_VERSION, &fixture.tag);
    let first = release_json(true, false, &fixture.tag, &records);
    let final_records = fixture.manifest_expected()?;
    let draft = release_json(true, false, &fixture.tag, &final_records);
    let published = release_json(false, true, &fixture.tag, &final_records);
    let output = fixture.run(&[first, draft, published], &fixture.commit, &fixture.commit)?;
    assert_success(&output);

    let manifest = fs::read_to_string(&fixture.manifest_copy)?;
    let accepted_manifest = fixture.accepted_directory.join(MANIFEST_NAME);
    let accepted_checksum = fixture.accepted_directory.join(MANIFEST_CHECKSUM_NAME);
    let acceptance = fixture.accepted_directory.join(ACCEPTANCE_NAME);
    for (target, _source_directory, _bytes, _digest, _sidecar_digest) in
        TARGET_FIXTURES.iter().copied()
    {
        let name = format!("velnor-actions-{RELEASE_VERSION}-{target}");
        let url = asset_url(&fixture.tag, &name);
        assert!(manifest.contains(&url));
        assert!(fs::read_to_string(&accepted_manifest)?.contains(&url));
    }
    assert!(manifest.contains(&format!("\"commit\":\"{}\"", fixture.commit)));
    assert_eq!(
        fs::read(&accepted_manifest)?,
        fs::read(&fixture.manifest_copy)?
    );
    assert_eq!(
        fs::read(&accepted_checksum)?,
        fs::read(&fixture.manifest_checksum_copy)?
    );
    let receipt = fs::read_to_string(acceptance)?;
    assert!(receipt.contains("\"immutable\":true"));
    assert!(receipt.contains("\"release_id\":741852963"));
    assert!(receipt.contains(&format!("\"source_commit\":\"{}\"", fixture.commit)));
    assert!(receipt.contains("\"digest\":\"sha256:"));
    assert!(receipt.contains(&asset_url(&fixture.tag, MANIFEST_NAME)));
    assert_eq!(fs::read_to_string(&fixture.release_index)?, "3");
    assert!(fixture.tag_path.exists());
    assert!(fixture.release_published.exists());

    let gh_calls = fs::read_to_string(&fixture.calls)?;
    assert!(gh_calls.contains("releases/741852963"));
    assert!(!gh_calls.contains("releases/tags/"));
    assert!(!gh_calls.contains("immutable-releases"));
    assert_eq!(gh_calls.matches("--draft=false").count(), 1);
    assert_eq!(gh_calls.matches("\"view\"").count(), 1);
    let uploads = fs::read_to_string(&fixture.upload_log)?;
    let manifest_upload = uploads
        .find(MANIFEST_CHECKSUM_NAME)
        .ok_or("manifest checksum was not uploaded")?;
    let publication = uploads
        .find("publish\n")
        .ok_or("release was not published")?;
    assert!(manifest_upload < publication);
    assert!(!uploads[publication..].contains("draft-upload:"));
    Ok(())
}

#[test]
fn publisher_fails_closed_for_bad_draft_assets_and_source() -> Result<(), Box<dyn Error>> {
    for case in [
        "missing-url",
        "wrong-url",
        "wrong-source",
        "wrong-event-source",
        "wrong-target-set",
        "missing-third-target-set",
        "wrong-hash",
        "unsafe-path",
        "wrong-upload-state",
    ] {
        let fixture = PublisherFixture::new(case)?;
        let mut records = asset_records(RELEASE_VERSION, &fixture.tag);
        let mut tag_sha = fixture.commit.clone();
        let mut event_sha = fixture.commit.clone();
        let reason = match case {
            "missing-url" => {
                records = super::asset_records_without_url(RELEASE_VERSION, &fixture.tag);
                "manifest_validation_failed"
            }
            "wrong-url" => {
                records[0] =
                    records[0].replace(&fixture.tag, &format!("generator-{}", "ff".repeat(20)));
                "manifest_validation_failed"
            }
            "wrong-source" => {
                tag_sha = "cd".repeat(20);
                "github_tag_create_source_mismatch"
            }
            "wrong-event-source" => {
                event_sha = "ef".repeat(20);
                "release_source_not_current_main"
            }
            "wrong-target-set" => {
                for record in &mut records {
                    if record.contains(MACOS_X86_64_TARGET) {
                        *record =
                            record.replace(MACOS_X86_64_TARGET, "x86_64-apple-darwin-invalid");
                    }
                }
                "manifest_validation_failed"
            }
            "missing-third-target-set" => {
                records.retain(|record| !record.contains(MACOS_X86_64_TARGET));
                "manifest_validation_failed"
            }
            "wrong-hash" => {
                records[0] = records[0].replace(super::LINUX_SHA, &"0".repeat(64));
                "manifest_validation_failed"
            }
            "unsafe-path" => {
                records[0] = records[0].replace(
                    &format!("velnor-actions-{RELEASE_VERSION}-{LINUX_TARGET}"),
                    "../escape",
                );
                "manifest_validation_failed"
            }
            "wrong-upload-state" => {
                records[0] = records[0].replace("\"state\":\"uploaded\"", "\"state\":\"starter\"");
                "manifest_validation_failed"
            }
            _ => return Err(format!("unknown case: {case}").into()),
        };
        let response = release_json(true, false, &fixture.tag, &records);
        let output = fixture.run(&[response], &tag_sha, &event_sha)?;
        assert_failure(&output, reason);
        assert!(!fixture.accepted_directory.exists());
        assert!(!fixture.manifest_copy.exists());
        let calls = fs::read_to_string(&fixture.calls).unwrap_or_default();
        assert!(!calls.contains("--draft=false"));
        assert!(!calls.contains("immutable-releases"));
    }
    Ok(())
}

#[test]
fn publisher_rejects_untrusted_workflow_context_before_release_creation()
-> Result<(), Box<dyn Error>> {
    for (event, reference, workflow_ref) in [
        (
            "pull_request",
            "refs/pull/42/merge",
            "tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/pull/42/merge",
        ),
        (
            "workflow_dispatch",
            "refs/heads/feature",
            "tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/heads/feature",
        ),
    ] {
        let fixture = PublisherFixture::new("untrusted-workflow")?;
        let output = fixture.run_with_workflow(
            &[],
            &fixture.commit,
            &fixture.commit,
            event,
            reference,
            workflow_ref,
        )?;
        assert_failure(&output, "release_requires_exact_main_workflow_dispatch");
        assert!(!fixture.release_created.exists());
        assert!(!fixture.release_published.exists());
        assert!(!fixture.accepted_directory.exists());
        assert!(!fixture.calls.exists());
    }
    Ok(())
}

#[test]
fn mutable_published_release_is_rejected_without_acceptance_metadata() -> Result<(), Box<dyn Error>>
{
    let fixture = PublisherFixture::new("mutable-published")?;
    let records = asset_records(RELEASE_VERSION, &fixture.tag);
    let first = release_json(true, false, &fixture.tag, &records);
    let final_records = fixture.manifest_expected()?;
    let draft = release_json(true, false, &fixture.tag, &final_records);
    let published = release_json(false, false, &fixture.tag, &final_records);
    let output = fixture.run(&[first, draft, published], &fixture.commit, &fixture.commit)?;
    assert_failure(
        &output,
        "manifest_validation_failed:published_release_not_immutable",
    );
    assert!(fixture.release_published.exists());
    assert!(!fixture.accepted_directory.exists());
    Ok(())
}
