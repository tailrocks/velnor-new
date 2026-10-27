// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::publication_retry::{
    PublicationAction, PublicationIdentity, PublicationRejection, PublicationState,
    publication_action,
};
use velnor_actions_contract::workflow::cache_context::{ExecutionContextV1, MeasuredU64};
use velnor_actions_contract::workflow::cache_identity::parse_strict_record;
use velnor_actions_contract::workflow::cache_policy::RequestedCoverageV1;

use super::fixtures::{
    digest_fill, fixture_compatibility, fixture_context, fixture_coverage, fixture_lock,
    fixture_lock_asset, fixture_lock_config, fixture_publication_identity, fixture_recipe,
    fixture_unknown, publication_available,
};

pub(super) fn rejects_malformed_coverage() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_coverage()?;
    bad.schema = 2;
    assert!(bad.validate().is_err());
    let mut bad = fixture_coverage()?;
    bad.generation.compatibility_digest = digest_fill('0')?;
    assert!(bad.validate().is_err());
    let mut bad = fixture_coverage()?;
    bad.ordered_recipe_ids.clear();
    assert!(bad.validate().is_err());
    let text = String::from_utf8(fixture_coverage()?.to_canonical_bytes()?)?;
    let tampered = text.replacen("\"schema\":1", "\"schema\":1,\"extra\":true", 1);
    assert!(RequestedCoverageV1::parse_json(&tampered).is_err());
    let duplicated = text.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
    assert!(RequestedCoverageV1::parse_json(&duplicated).is_err());
    let mut bad_recipe = fixture_recipe("unit-a")?;
    bad_recipe.features = vec!["z".to_owned(), "a".to_owned()];
    bad_recipe.recipe_id = bad_recipe.computed_recipe_id()?;
    assert!(bad_recipe.validate().is_err());
    let mut bad_path = fixture_recipe("unit-a")?;
    "/tmp".clone_into(&mut bad_path.cwd_relative);
    bad_path.recipe_id = bad_path.computed_recipe_id()?;
    assert!(bad_path.validate().is_err());
    let mut bad_profile = fixture_recipe("unit-a")?;
    bad_profile.cargo_profile.clear();
    bad_profile.recipe_id = bad_profile.computed_recipe_id()?;
    assert!(bad_profile.validate().is_err());
    let mut bad_toolchain = fixture_compatibility()?;
    bad_toolchain.toolchain_identity.clear();
    assert!(bad_toolchain.validate().is_err());
    Ok(())
}

pub(super) fn rejects_malformed_context() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_context()?;
    bad.schema = 2;
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.compiler_adapters.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.cargo_home.clone_from(&bad.owned_root);
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.lanes[0].lane_id.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.lanes[0].build_dir = Some("/absolute".to_owned());
    assert!(bad.validate().is_err());
    let mut bad = fixture_context()?;
    bad.compiler_adapters[0].executable_identity.clear();
    assert!(bad.validate().is_err());
    let text = String::from_utf8(fixture_context()?.to_canonical_bytes()?)?;
    let tampered = text.replacen("\"schema\":1", "\"schema\":1,\"extra\":true", 1);
    assert!(ExecutionContextV1::parse_json(&tampered).is_err());
    Ok(())
}

pub(super) fn rejects_malformed_publication() {
    assert!(parse_strict_record::<PublicationIdentity>("{}").is_err());
    assert!(
        parse_strict_record::<PublicationIdentity>(
            "{\"source_sha\":\"c\",\"receipt_digest\":\"r\",\"assets\":{},\"extra\":true}"
        )
        .is_err()
    );
    let expected = fixture_publication_identity();
    let available = publication_available(&expected);
    assert_eq!(
        publication_action(&expected, &PublicationState::Absent, false, &available),
        PublicationAction::Reject(PublicationRejection::StaleCandidate)
    );
    let wrong_tag = PublicationState::TagOnly {
        source_sha: "d".repeat(40),
        original_receipt_digest: Some(expected.receipt_digest.clone()),
    };
    assert_eq!(
        publication_action(&expected, &wrong_tag, true, &available),
        PublicationAction::Reject(PublicationRejection::ConflictingTag)
    );
    let unknown_origin = PublicationState::TagOnly {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: None,
    };
    assert_eq!(
        publication_action(&expected, &unknown_origin, true, &available),
        PublicationAction::Reject(PublicationRejection::UnknownPublicationOrigin)
    );
    let wrong_receipt = PublicationState::TagOnly {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: Some("other-receipt".to_owned()),
    };
    assert_eq!(
        publication_action(&expected, &wrong_receipt, true, &available),
        PublicationAction::Reject(PublicationRejection::ConflictingIdentity)
    );
    let mut short_available = BTreeSet::new();
    short_available.insert("release.json".to_owned());
    assert_eq!(
        publication_action(&expected, &PublicationState::Absent, true, &short_available),
        PublicationAction::Reject(PublicationRejection::UnavailableOriginalAssets)
    );
    let mutable = PublicationState::Published {
        identity: expected.clone(),
        immutable: false,
    };
    assert_eq!(
        publication_action(&expected, &mutable, true, &available),
        PublicationAction::Reject(PublicationRejection::MutablePublishedRelease)
    );
    let mut drifted = expected.clone();
    "other-receipt".clone_into(&mut drifted.receipt_digest);
    let conflicting = PublicationState::Published {
        identity: drifted,
        immutable: true,
    };
    assert_eq!(
        publication_action(&expected, &conflicting, true, &available),
        PublicationAction::Reject(PublicationRejection::ConflictingIdentity)
    );
}

pub(super) fn rejects_malformed_lock() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_lock()?;
    bad.schema = 2;
    assert!(bad.validate(".velnor/deps.lock").is_err());
    let mut bad = fixture_lock()?;
    bad.release_assets.push(fixture_lock_asset("tool-cache"));
    assert!(bad.validate(".velnor/deps.lock").is_err());
    let mut bad = fixture_lock()?;
    "xyz".clone_into(&mut bad.release_assets[0].commit);
    assert!(bad.validate(".velnor/deps.lock").is_err());
    let mut bad = fixture_lock()?;
    bad.release_assets[0].sha256.clone_from(&"g".repeat(64));
    assert!(bad.validate(".velnor/deps.lock").is_err());
    let mut bad = fixture_lock()?;
    "not-a-repository".clone_into(&mut bad.release_assets[0].repository);
    assert!(bad.validate(".velnor/deps.lock").is_err());
    let mut bad = fixture_lock()?;
    bad.release_assets[0].extract_to = Some("../escape".to_owned());
    assert!(bad.validate(".velnor/deps.lock").is_err());
    let mut drifted_config = fixture_lock_config();
    "v9.9.9".clone_into(&mut drifted_config.release_assets[0].tag);
    assert!(
        fixture_lock()?
            .check_matches(&drifted_config, ".velnor/deps.lock")
            .is_err()
    );
    let mut extra_lock = fixture_lock()?;
    extra_lock
        .release_assets
        .push(fixture_lock_asset("extra-tool"));
    assert!(
        extra_lock
            .check_matches(&fixture_lock_config(), ".velnor/deps.lock")
            .is_err()
    );
    Ok(())
}

pub(super) fn rejects_collapsed_counters() -> Result<(), Box<dyn Error>> {
    let both = MeasuredU64 {
        value: Some(3),
        unknown_reason: fixture_unknown()?.unknown_reason,
    };
    assert!(both.validate().is_err());
    let neither = MeasuredU64 {
        value: None,
        unknown_reason: None,
    };
    assert!(neither.validate().is_err());
    assert!(MeasuredU64::parse_json("{\"value\":null,\"unknown_reason\":null}").is_err());
    let unknown_text = String::from_utf8(fixture_unknown()?.to_canonical_bytes()?)?;
    let collapsed = unknown_text.replacen("\"value\":null", "\"value\":1", 1);
    assert!(MeasuredU64::parse_json(&collapsed).is_err());
    Ok(())
}
