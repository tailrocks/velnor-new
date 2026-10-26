// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::publication_retry::{
    PublicationAction, PublicationState, publication_action,
};
use velnor_actions_contract::workflow::cache_context::MeasuredU64;

use super::fixtures::{
    digest_fill, fixture_compatibility, fixture_coverage, fixture_lock, fixture_lock_config,
    fixture_publication_identity, fixture_recipe, fixture_unknown, publication_available,
    rebuild_coverage, retune_recipe,
};

pub(super) fn check_lock_identity() -> Result<(), Box<dyn Error>> {
    let lock = fixture_lock()?;
    assert!(!lock.is_empty());
    assert_eq!(
        lock.repositories().into_iter().collect::<Vec<_>>(),
        vec!["tailrocks/velnor-new"]
    );
    lock.check_matches(&fixture_lock_config(), ".velnor/deps.lock")?;
    let first = rebuild_coverage(
        fixture_compatibility()?,
        vec![fixture_recipe("unit-a")?, fixture_recipe("unit-b")?],
        digest_fill('8')?,
    )?;
    let second = rebuild_coverage(
        fixture_compatibility()?,
        vec![fixture_recipe("unit-a")?, fixture_recipe("unit-b")?],
        digest_fill('0')?,
    )?;
    assert_ne!(first.coverage_digest()?, second.coverage_digest()?);
    assert_eq!(
        first.coverage_digest()?,
        fixture_coverage()?.coverage_digest()?
    );
    Ok(())
}

pub(super) fn check_feature_identity() -> Result<(), Box<dyn Error>> {
    let base = fixture_recipe("unit-a")?;
    let mut varied = base.clone();
    varied.features = vec!["other".to_owned()];
    let varied = retune_recipe(varied)?;
    assert_ne!(base.recipe_id, varied.recipe_id);
    let mut toggled = base.clone();
    toggled.default_features = true;
    let toggled = retune_recipe(toggled)?;
    assert_ne!(base.recipe_id, toggled.recipe_id);
    let mut cleared = base.clone();
    cleared.features = Vec::new();
    let cleared = retune_recipe(cleared)?;
    assert_ne!(base.recipe_id, cleared.recipe_id);
    Ok(())
}

pub(super) fn check_path_identity() -> Result<(), Box<dyn Error>> {
    let base = fixture_recipe("unit-a")?;
    let mut moved_recipe = base.clone();
    "crates/demo".clone_into(&mut moved_recipe.cwd_relative);
    let moved_recipe = retune_recipe(moved_recipe)?;
    assert_ne!(base.recipe_id, moved_recipe.recipe_id);
    let base_digest = fixture_compatibility()?.digest()?;
    let mut relaid = fixture_compatibility()?;
    relaid.path_layout_digest = digest_fill('0')?;
    assert_ne!(base_digest, relaid.digest()?);
    let mut reschemed = fixture_compatibility()?;
    reschemed.lane_scheme_digest = digest_fill('0')?;
    assert_ne!(base_digest, reschemed.digest()?);
    assert_eq!(base_digest, fixture_compatibility()?.digest()?);
    Ok(())
}

pub(super) fn check_profile_identity() -> Result<(), Box<dyn Error>> {
    let base = fixture_recipe("unit-a")?;
    let mut released = base.clone();
    "release".clone_into(&mut released.cargo_profile);
    let released = retune_recipe(released)?;
    assert_ne!(base.recipe_id, released.recipe_id);
    let mut nextested = base.clone();
    "ci".clone_into(&mut nextested.nextest_profile);
    let nextested = retune_recipe(nextested)?;
    assert_ne!(base.recipe_id, nextested.recipe_id);
    assert_ne!(released.recipe_id, nextested.recipe_id);
    Ok(())
}

pub(super) fn check_toolchain_identity() -> Result<(), Box<dyn Error>> {
    let base_digest = fixture_compatibility()?.digest()?;
    let mut retooled = fixture_compatibility()?;
    "stable-1.99.2".clone_into(&mut retooled.toolchain_identity);
    assert_ne!(base_digest, retooled.digest()?);
    let mut recompiled = fixture_compatibility()?;
    "rustc-1.99.2-x86_64".clone_into(&mut recompiled.compiler_identity);
    assert_ne!(base_digest, recompiled.digest()?);
    assert_eq!(base_digest, fixture_compatibility()?.digest()?);
    Ok(())
}

pub(super) fn check_native_identity() -> Result<(), Box<dyn Error>> {
    let base_digest = fixture_compatibility()?.digest()?;
    let mut reenvironmented = fixture_compatibility()?;
    reenvironmented.native_environment_digest = digest_fill('0')?;
    assert_ne!(base_digest, reenvironmented.digest()?);
    let base_recipe = fixture_recipe("unit-a")?;
    let mut reinputed = base_recipe.clone();
    reinputed.native_input_digest = digest_fill('0')?;
    let reinputed = retune_recipe(reinputed)?;
    assert_ne!(base_recipe.recipe_id, reinputed.recipe_id);
    Ok(())
}

pub(super) fn check_unknown_counters() -> Result<(), Box<dyn Error>> {
    let zero = MeasuredU64 {
        value: Some(0),
        unknown_reason: None,
    };
    zero.validate()?;
    let unknown = fixture_unknown()?;
    let zero_bytes = zero.to_canonical_bytes()?;
    let unknown_bytes = unknown.to_canonical_bytes()?;
    assert_ne!(zero_bytes, unknown_bytes);
    let zero_text = String::from_utf8(zero_bytes)?;
    let unknown_text = String::from_utf8(unknown_bytes)?;
    assert!(zero_text.contains("\"value\":0"));
    assert!(unknown_text.contains("\"value\":null"));
    assert_eq!(zero, MeasuredU64::parse_json(&zero_text)?);
    assert_eq!(unknown, MeasuredU64::parse_json(&unknown_text)?);
    Ok(())
}

pub(super) fn check_publication_decisions() {
    let expected = fixture_publication_identity();
    let available = publication_available(&expected);
    assert_eq!(
        publication_action(&expected, &PublicationState::Absent, true, &available),
        PublicationAction::CreateTag
    );
    let draft = PublicationState::Draft {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: Some(expected.receipt_digest.clone()),
        assets: expected.assets.clone(),
    };
    assert_eq!(
        publication_action(&expected, &draft, true, &available),
        PublicationAction::CompleteDraft {
            missing_assets: Vec::new(),
        }
    );
    let mut partial_assets = BTreeMap::new();
    partial_assets.insert("release.json".to_owned(), "digest-release".to_owned());
    let partial = PublicationState::Draft {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: Some(expected.receipt_digest.clone()),
        assets: partial_assets,
    };
    assert_eq!(
        publication_action(&expected, &partial, true, &available),
        PublicationAction::CompleteDraft {
            missing_assets: vec!["payload.tar.gz".to_owned()],
        }
    );
    let published = PublicationState::Published {
        identity: expected.clone(),
        immutable: true,
    };
    assert_eq!(
        publication_action(&expected, &published, true, &available),
        PublicationAction::ReconcileDistribution
    );
}
