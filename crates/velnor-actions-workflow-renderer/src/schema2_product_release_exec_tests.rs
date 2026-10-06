use std::error::Error;

use self::fixture::{Fixture, contains_mutation};
use super::{Family, family, test_pins};

#[path = "schema2_product_release_exec_fixture.rs"]
mod fixture;

#[test]
fn absent_family_is_marked_build_without_mutating_github() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "absent")?;
    let result = fixture.run(family::prepare_script(Family::Binary, &test_pins())?, None)?;
    assert!(result.success, "{}", result.stderr);
    assert!(result.output.contains("action=build"));
    assert!(!contains_mutation(&result.calls));
    Ok(())
}

#[test]
fn draft_and_orphan_tags_fail_closed_without_mutation() -> Result<(), Box<dyn Error>> {
    for mode in ["draft", "orphan"] {
        let fixture = Fixture::new(Family::Binary, mode)?;
        let result = fixture.run(family::prepare_script(Family::Binary, &test_pins())?, None)?;
        assert!(!result.success, "mode {mode} unexpectedly passed");
        assert!(!contains_mutation(&result.calls));
    }
    Ok(())
}

#[test]
fn wrong_tag_target_fails_closed_without_mutation() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "wrong-target")?;
    let result = fixture.run(family::prepare_script(Family::Binary, &test_pins())?, None)?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("tag does not resolve to the exact source commit"),
        "{}",
        result.stderr
    );
    assert!(!contains_mutation(&result.calls));
    Ok(())
}

#[test]
fn stale_main_fails_publisher_before_tag_or_release_mutation() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "stale")?;
    fixture.create_build_assets()?;
    let result = fixture.run(
        family::publish_script(Family::Binary, &test_pins())?,
        Some("build"),
    )?;
    assert!(!result.success);
    assert!(result.stderr.contains("source is no longer the main tip"));
    assert!(!contains_mutation(&result.calls));
    Ok(())
}

#[test]
fn changed_latest_ci_attempt_fails_before_tag_creation() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "attempt-changed")?;
    fixture.create_build_assets()?;
    let result = fixture.run(
        family::publish_script(Family::Binary, &test_pins())?,
        Some("build"),
    )?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("latest CI attempt changed during eligibility check")
    );
    assert!(!contains_mutation(&result.calls));
    Ok(())
}

#[test]
fn publisher_creates_exact_tag_after_eligibility_then_publishes_verified_assets()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "absent")?;
    fixture.create_build_assets()?;
    let result = fixture.run(
        family::publish_script(Family::Binary, &test_pins())?,
        Some("build"),
    )?;
    assert!(result.success, "{}", result.stderr);
    assert_order(&result.calls, "/attempts/1/jobs?per_page=100", "git/refs")?;
    assert_order(&result.calls, "git/refs", "release create")?;
    assert_order(&result.calls, "release create", "release upload")?;
    assert_order(&result.calls, "release upload", "release edit")?;
    assert!(result.calls.contains("--source-ref refs/heads/main"));
    assert!(result.calls.contains("--signer-digest"));
    assert!(result.calls.contains("--source-digest"));
    Ok(())
}

#[test]
fn complete_publisher_fails_closed_on_release_verification_errors() -> Result<(), Box<dyn Error>> {
    for (failure, diagnostic) in [
        ("release-verify", "release signature verification failed"),
        ("download", "release asset download failed"),
        ("checksum", "release asset checksum verification failed"),
        ("asset-verify", "release asset verification failed"),
        (
            "attestation-verify",
            "asset attestation verification failed",
        ),
    ] {
        let fixture = Fixture::new(Family::Binary, "complete")?;
        let result = fixture.run_with_failure(
            family::publish_script(Family::Binary, &test_pins())?,
            Some("complete"),
            Some(failure),
        )?;
        assert!(!result.success, "{failure} unexpectedly passed");
        assert!(
            result.stderr.contains(diagnostic),
            "{failure}: {}",
            result.stderr
        );
        assert!(
            !result.output.contains("action=complete"),
            "{failure} emitted complete: {}",
            result.output
        );
        assert!(
            !contains_mutation(&result.calls),
            "{failure} reached a mutating command: {}",
            result.calls
        );
    }
    Ok(())
}

#[test]
fn complete_publisher_fails_closed_on_authoritative_read_errors() -> Result<(), Box<dyn Error>> {
    for (mode, failure, diagnostic) in [
        ("absent", "main-read", "main commit response is invalid"),
        ("absent", "ci-read", "CI run response is invalid"),
        ("absent", "required-read", "Required job lookup failed"),
        (
            "complete",
            "release-list-read",
            "release list request failed",
        ),
        (
            "absent",
            "matching-refs-read",
            "matching release tag lookup failed",
        ),
        ("complete", "tag-read", "release tag lookup failed"),
    ] {
        let fixture = Fixture::new(Family::Binary, mode)?;
        let result = fixture.run_with_failure(
            family::publish_script(Family::Binary, &test_pins())?,
            Some("complete"),
            Some(failure),
        )?;
        assert!(!result.success, "{failure} unexpectedly passed");
        assert!(
            result.stderr.contains(diagnostic),
            "{failure}: {}",
            result.stderr
        );
        assert!(
            !result.output.contains("action=complete"),
            "{failure} emitted complete: {}",
            result.output
        );
        assert!(
            !contains_mutation(&result.calls),
            "{failure} reached a mutating command: {}",
            result.calls
        );
    }
    Ok(())
}

#[test]
fn mutable_post_publish_result_fails_qualification_after_upload() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "mutable")?;
    fixture.create_build_assets()?;
    let result = fixture.run(
        family::publish_script(Family::Binary, &test_pins())?,
        Some("build"),
    )?;
    assert!(!result.success);
    assert!(
        result
            .stderr
            .contains("not the exact immutable family release")
    );
    assert!(result.calls.contains("release edit"));
    Ok(())
}

#[test]
fn draft_creation_failure_leaves_an_orphan_tag_that_retry_rejects() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Family::Binary, "draft-fails")?;
    fixture.create_build_assets()?;
    let publish = fixture.run(
        family::publish_script(Family::Binary, &test_pins())?,
        Some("build"),
    )?;
    assert!(!publish.success);
    assert!(publish.calls.contains("git/refs"));
    assert!(publish.calls.contains("release create"));
    assert!(!publish.calls.contains("release upload"));

    let previous_calls = publish.calls.len();
    let retry = fixture.run(family::prepare_script(Family::Binary, &test_pins())?, None)?;
    assert!(!retry.success);
    assert!(
        retry
            .stderr
            .contains("tag exists without a published release")
    );
    assert!(!contains_mutation(&retry.calls[previous_calls..]));
    Ok(())
}

fn assert_order(calls: &str, earlier: &str, later: &str) -> Result<(), Box<dyn Error>> {
    let earlier_at = calls.find(earlier).ok_or(format!("missing {earlier}"))?;
    let later_at = calls.find(later).ok_or(format!("missing {later}"))?;
    if earlier_at >= later_at {
        return Err(format!("{earlier} did not precede {later}").into());
    }
    Ok(())
}
