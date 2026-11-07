// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::{Fixture, FixtureProfile, PromotionCase};
use std::error::Error;

#[test]
fn promotion_boundary_tags_the_attested_source_digest_without_uploading_blobs()
-> Result<(), Box<dyn Error>> {
    super::run_isolated(
        "promotion::promotion_boundary_tags_the_attested_source_digest_without_uploading_blobs",
        || {
            for case in [
                PromotionCase::Valid,
                PromotionCase::ValidEncodedPath,
                PromotionCase::MatchingExisting,
            ] {
                let fixture = Fixture::new_for(FixtureProfile::Runner)?;
                fixture.set_promotion_case(case)?;
                let before = fixture.promotion_state()?;
                let output = fixture.invoke_promotion_boundary()?;
                assert!(
                    output.status.success(),
                    "{}; fixture: {:?}",
                    String::from_utf8_lossy(&output.stderr),
                    fixture.last_fixture_error()?
                );
                let after = fixture.promotion_state()?;
                assert_eq!(after["source_tag"], before["source_tag"]);
                assert_eq!(after["blobs"], before["blobs"]);
                assert_eq!(
                    after["version_tag"]["digest"],
                    before["source_tag"]["digest"]
                );
                let operations = after["operations"].as_array().ok_or("operations missing")?;
                let tags: Vec<_> = operations
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|v| v.starts_with("tag "))
                    .collect();
                if matches!(case, PromotionCase::Valid | PromotionCase::ValidEncodedPath) {
                    assert_eq!(
                        tags,
                        [format!(
                            "tag {}@{} 1.2.3",
                            fixture.profile.repository(),
                            fixture.expected_digest
                        )]
                    );
                } else {
                    assert_eq!(tags.len(), 0);
                }
                assert!(
                    operations
                        .iter()
                        .filter_map(|v| v.as_str())
                        .all(|v| !v.starts_with("push "))
                );
            }
            Ok(())
        },
    )
}

#[test]
fn promotion_boundary_rejects_unapproved_authority_and_conflicting_tags_without_registry_writes()
-> Result<(), Box<dyn Error>> {
    super::run_isolated(
        "promotion::promotion_boundary_rejects_unapproved_authority_and_conflicting_tags_without_registry_writes",
        || {
            for (case, expected_error) in [
                (PromotionCase::WrongPublisherRun, "image_promotion_run"),
                (
                    PromotionCase::WrongAnnotatedTarget,
                    "image_promotion_annotated_tag",
                ),
                (
                    PromotionCase::WrongTagObjectType,
                    "image_promotion_annotated_tag",
                ),
                (
                    PromotionCase::WrongVersionDeclaration,
                    "image_promotion_version",
                ),
                (
                    PromotionCase::WrongImageDeclaration,
                    "image_promotion_image",
                ),
                (
                    PromotionCase::FailedRequiredCi,
                    "image_publication_ci_required_job",
                ),
                (
                    PromotionCase::AttestationFailure,
                    "image_promotion_attestation",
                ),
                (
                    PromotionCase::ExistingConflict,
                    "image_publication_tag_conflict",
                ),
            ] {
                let fixture = Fixture::new_for(FixtureProfile::Runner)?;
                fixture.set_promotion_case(case)?;
                let before = fixture.promotion_state()?;
                let output = if case == PromotionCase::AttestationFailure {
                    fixture.invoke_promotion()?
                } else {
                    fixture.invoke_promotion_boundary()?
                };
                assert!(
                    !output.status.success(),
                    "promotion unexpectedly accepted {case:?}"
                );
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(stderr.contains(expected_error), "{case:?}: {stderr}");
                let after = fixture.promotion_state()?;
                assert_eq!(after["source_tag"], before["source_tag"], "{case:?}");
                assert_eq!(after["version_tag"], before["version_tag"], "{case:?}");
                assert_eq!(after["blobs"], before["blobs"], "{case:?}");
                assert!(
                    after["operations"]
                        .as_array()
                        .ok_or("operations missing")?
                        .iter()
                        .filter_map(|v| v.as_str())
                        .all(|v| !v.starts_with("tag ") && !v.starts_with("push ")),
                    "{case:?}"
                );
            }
            Ok(())
        },
    )
}

#[test]
fn promotion_boundary_rejects_attested_digest_for_the_wrong_native_role()
-> Result<(), Box<dyn Error>> {
    super::run_isolated(
        "promotion::promotion_boundary_rejects_attested_digest_for_the_wrong_native_role",
        || {
            let fixture = Fixture::new_for_image_kind(FixtureProfile::Runner, "dind")?;
            fixture.set_promotion_case(PromotionCase::Valid)?;
            let before = fixture.promotion_state()?;
            let output = fixture.invoke_promotion_boundary()?;
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("image_promotion_image_kind"));
            let after = fixture.promotion_state()?;
            assert_eq!(after["source_tag"], before["source_tag"]);
            assert_eq!(after["version_tag"], before["version_tag"]);
            assert_eq!(after["blobs"], before["blobs"]);
            assert!(
                after["operations"]
                    .as_array()
                    .ok_or("operations missing")?
                    .iter()
                    .filter_map(|value| value.as_str())
                    .all(|value| !value.starts_with("tag ") && !value.starts_with("push "))
            );
            Ok(())
        },
    )
}

#[test]
fn promotion_boundary_reserved_job_repository_requires_the_job_role() -> Result<(), Box<dyn Error>>
{
    super::run_isolated(
        "promotion::promotion_boundary_reserved_job_repository_requires_the_job_role",
        || {
            for (kind, revision, accepted) in [
                ("job", "a".repeat(40), true),
                ("runner", "a".repeat(40), false),
                ("job", "b".repeat(40), false),
            ] {
                let fixture =
                    Fixture::new_for_image_identity(FixtureProfile::Runner, kind, &revision)?;
                fixture.set_promotion_case(PromotionCase::ReservedJobRepository)?;
                let before = fixture.promotion_state()?;
                let output = fixture.invoke_promotion_boundary()?;
                assert_eq!(output.status.success(), accepted, "{kind}");
                let after = fixture.promotion_state()?;
                assert_eq!(after["source_tag"], before["source_tag"], "{kind}");
                assert_eq!(after["blobs"], before["blobs"], "{kind}");
                if accepted {
                    assert_eq!(
                        after["version_tag"]["digest"],
                        before["source_tag"]["digest"]
                    );
                } else {
                    assert!(
                        String::from_utf8_lossy(&output.stderr)
                            .contains("image_promotion_image_kind")
                    );
                    assert_eq!(after["version_tag"], before["version_tag"]);
                }
                assert!(
                    after["operations"]
                        .as_array()
                        .ok_or("operations missing")?
                        .iter()
                        .filter_map(|value| value.as_str())
                        .all(|value| !value.starts_with("push ")
                            && (accepted || !value.starts_with("tag ")))
                );
            }
            Ok(())
        },
    )
}

#[test]
fn promotion_boundary_generic_image_without_native_role_label_can_be_promoted()
-> Result<(), Box<dyn Error>> {
    super::run_isolated(
        "promotion::promotion_boundary_generic_image_without_native_role_label_can_be_promoted",
        || {
            let fixture = Fixture::new_unlabeled_promotion()?;
            fixture.set_promotion_case(PromotionCase::GenericUnlabeled)?;
            let before = fixture.promotion_state()?;
            let output = fixture.invoke_promotion_boundary()?;
            assert!(
                output.status.success(),
                "{}; fixture: {:?}",
                String::from_utf8_lossy(&output.stderr),
                fixture.last_fixture_error()?
            );
            let after = fixture.promotion_state()?;
            assert_eq!(after["source_tag"], before["source_tag"]);
            assert_eq!(after["blobs"], before["blobs"]);
            assert_eq!(
                after["version_tag"]["digest"],
                before["source_tag"]["digest"]
            );
            Ok(())
        },
    )
}
