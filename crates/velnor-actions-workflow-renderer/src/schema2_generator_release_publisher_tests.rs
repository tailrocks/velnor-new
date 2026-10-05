use super::{
    LINUX_TARGET, MACOS_TARGET, MANIFEST_NAME, RELEASE_VERSION, assert_failure, assert_success,
    asset_record, asset_records, asset_url, output_text, release_json,
};
use std::error::Error;
use std::fs;

#[path = "schema2_generator_release_publisher_fixture.rs"]
mod fixture;
use self::fixture::PublisherFixture;

#[test]
fn publisher_uses_observed_urls_and_publishes_only_after_full_api_verification()
-> Result<(), Box<dyn Error>> {
    let fixture = PublisherFixture::new("publish-valid")?;
    let records = asset_records(RELEASE_VERSION, &fixture.tag);
    let first = release_json(true, false, &fixture.tag, &records);
    let final_records = fixture.manifest_expected()?;
    let draft = release_json(true, false, &fixture.tag, &final_records);
    let published = release_json(false, true, &fixture.tag, &final_records);
    let output = fixture.run(
        &[first, draft, published],
        &fixture.commit,
        &fixture.commit,
        "enabled",
    )?;
    assert_success(&output);
    let manifest = fs::read_to_string(&fixture.manifest_copy)?;
    for target in [LINUX_TARGET, MACOS_TARGET] {
        let name = format!("velnor-actions-{RELEASE_VERSION}-{target}");
        assert!(manifest.contains(&asset_url(&fixture.tag, &name)));
    }
    assert!(manifest.contains(&format!("\"commit\":\"{}\"", fixture.commit)));
    assert_eq!(fs::read_to_string(&fixture.release_index)?, "3");
    let mise_calls = fs::read_to_string(&fixture.mise_calls)?;
    assert!(mise_calls.contains("gh@2.102.0"));
    assert!(mise_calls.contains("exec"));
    let gh_calls = fs::read_to_string(&fixture.calls)?;
    assert!(gh_calls.contains("release"));
    assert!(gh_calls.contains("--draft=false"));
    assert!(gh_calls.contains("immutable-releases"));
    assert!(gh_calls.contains("releases/741852963"));
    assert!(!gh_calls.contains("releases/tags/"));
    assert!(fixture.tag_path.exists());
    assert!(fixture.release_published.exists());
    Ok(())
}

#[test]
fn publisher_fails_closed_for_invalid_api_metadata_and_source() -> Result<(), Box<dyn Error>> {
    for case in [
        "missing-url",
        "wrong-url",
        "wrong-source",
        "wrong-event-source",
        "wrong-target-set",
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
                records[2] = records[2].replace(MACOS_TARGET, "wrong-target");
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
        let output = fixture.run(
            &[response.clone(), response.clone(), response],
            &tag_sha,
            &event_sha,
            "enabled",
        )?;
        assert_failure(&output, reason);
        assert!(!fixture.manifest_copy.exists());
        let calls = fs::read_to_string(&fixture.calls).unwrap_or_default();
        assert!(!calls.contains("--draft=false"));
    }
    Ok(())
}

#[test]
fn publisher_refuses_mutable_release_policy_before_creating_source_tag()
-> Result<(), Box<dyn Error>> {
    let fixture = PublisherFixture::new("mutable-release-policy")?;
    let output = fixture.run(&[], &fixture.commit, &fixture.commit, "disabled")?;
    assert_failure(&output, "immutable_release_policy_disabled");
    assert!(!fixture.tag_path.exists());
    assert!(!fixture.release_created.exists());
    assert!(!fixture.release_published.exists());
    Ok(())
}

#[test]
fn publisher_refuses_unverifiable_immutability_before_creating_source_tag()
-> Result<(), Box<dyn Error>> {
    let fixture = PublisherFixture::new("unverifiable-release-policy")?;
    let output = fixture.run(&[], &fixture.commit, &fixture.commit, "unavailable")?;
    assert_failure(&output, "immutable_release_policy_unavailable");
    assert!(!fixture.tag_path.exists());
    assert!(!fixture.release_created.exists());
    assert!(!fixture.release_published.exists());
    Ok(())
}
