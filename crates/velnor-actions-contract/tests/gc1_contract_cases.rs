// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::config::{PrivateAssetSource, PrivateSourcesConfig};
use velnor_actions_contract::deps_lock::{DepsLock, LockedAsset};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::publication_retry::{
    PublicationAction, PublicationIdentity, PublicationRejection, PublicationState,
    publication_action,
};
use velnor_actions_contract::workflow::cache_context::{
    CacheCapability, CacheDriver, CompilerAdapterV1, ExecutionContextV1, JobBindingV1,
    LaneContextV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{
    CompatibilityV1, Gc1Family, GenerationId, parse_strict_record,
};
use velnor_actions_contract::workflow::cache_policy::{
    ImmutableObjectKind, ImmutableObjectV1, RecipeV1, RequestedCoverageV1,
};

fn digest_fill(fill: char) -> Result<Digest, ContractError> {
    Digest::parse(&format!("b3-{}", fill.to_string().repeat(64)))
}

fn fixture_compatibility() -> Result<CompatibilityV1, ContractError> {
    Ok(CompatibilityV1 {
        schema: 1,
        family: Gc1Family::State,
        repository_id: 42,
        workspace_id: digest_fill('a')?,
        host_os: "linux".to_owned(),
        host_arch: "x86_64".to_owned(),
        runner_image_identity: "ubuntu-24.04-20261001".to_owned(),
        abi_identity: "gnu-2.39".to_owned(),
        toolchain_identity: "stable-1.99.1".to_owned(),
        compiler_identity: "rustc-1.99.1-x86_64".to_owned(),
        native_environment_digest: digest_fill('b')?,
        target_environment_ids: vec!["x86_64-linux-gnu".to_owned()],
        path_layout_digest: digest_fill('c')?,
        lane_scheme_digest: digest_fill('d')?,
        archive_format: "tar.gz".to_owned(),
        observer_interface_version: 1,
        source_policy_digest: None,
        authorized_audience_digest: None,
    })
}

fn fixture_recipe(unit_id: &str) -> Result<RecipeV1, ContractError> {
    let mut environment = BTreeMap::new();
    environment.insert("CARGO_TERM_COLOR".to_owned(), "never".to_owned());
    let mut recipe = RecipeV1 {
        recipe_id: digest_fill('0')?,
        unit_id: unit_id.to_owned(),
        workspace_id: "workspace-1".to_owned(),
        package_id: "demo-crate".to_owned(),
        command_family: "build".to_owned(),
        lane_id: "lane-0".to_owned(),
        command_builder_id: "cargo-build".to_owned(),
        argv: vec![
            "cargo".to_owned(),
            "build".to_owned(),
            "-p".to_owned(),
            "demo-crate".to_owned(),
        ],
        cwd_relative: ".".to_owned(),
        executable_identity: "cargo-1.99.1".to_owned(),
        features: vec!["serde".to_owned()],
        default_features: false,
        cargo_profile: "dev".to_owned(),
        nextest_profile: "default".to_owned(),
        targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
        declared_nonsecret_environment: environment,
        warning_policy_digest: digest_fill('e')?,
        dependency_input_digest: digest_fill('f')?,
        workspace_dependency_digest: digest_fill('1')?,
        native_input_digest: digest_fill('2')?,
        declared_external_input_digest: digest_fill('3')?,
        required_output_roles: vec!["target-dir".to_owned()],
        source_bound_helper_digest: None,
    };
    recipe.recipe_id = recipe.computed_recipe_id()?;
    recipe.validate()?;
    Ok(recipe)
}

fn retune_recipe(mut recipe: RecipeV1) -> Result<RecipeV1, ContractError> {
    recipe.recipe_id = recipe.computed_recipe_id()?;
    recipe.validate()?;
    Ok(recipe)
}

fn fixture_object(object_id: &str) -> Result<ImmutableObjectV1, ContractError> {
    Ok(ImmutableObjectV1 {
        object_id: object_id.to_owned(),
        kind: ImmutableObjectKind::Source,
        approved_origin_id: "crates-io".to_owned(),
        immutable_locator: format!("sparse-index/{}", digest_fill('7')?.as_str()),
        content_digest: digest_fill('7')?,
        audience_digest: digest_fill('9')?,
    })
}

fn rebuild_coverage(
    compatibility: CompatibilityV1,
    recipes: Vec<RecipeV1>,
    dependency_inputs_digest: Digest,
) -> Result<RequestedCoverageV1, ContractError> {
    let mut ordered: Vec<String> = recipes
        .iter()
        .map(|recipe| recipe.recipe_id.as_str().to_owned())
        .collect();
    ordered.sort();
    let mut coverage = RequestedCoverageV1 {
        schema: 1,
        generation: GenerationId {
            family: Gc1Family::State,
            group_digest: digest_fill('4')?,
            compatibility_digest: digest_fill('0')?,
            coverage_digest: digest_fill('0')?,
            repair_revision: 0,
        },
        compatibility,
        ordered_recipe_ids: ordered,
        recipes,
        immutable_objects: vec![fixture_object("registry-index")?],
        active_variant_ids: vec!["default".to_owned()],
        dependency_inputs_digest,
    };
    let coverage_digest = coverage.coverage_digest()?;
    let compatibility_digest = coverage.compatibility.digest()?;
    coverage.generation.coverage_digest = coverage_digest;
    coverage.generation.compatibility_digest = compatibility_digest;
    coverage.validate()?;
    Ok(coverage)
}

fn fixture_coverage() -> Result<RequestedCoverageV1, ContractError> {
    rebuild_coverage(
        fixture_compatibility()?,
        vec![fixture_recipe("unit-a")?, fixture_recipe("unit-b")?],
        digest_fill('8')?,
    )
}

fn fixture_job() -> Result<JobBindingV1, ContractError> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 42,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 123,
        run_attempt: 1,
        job_id: 456,
        job_key: "reader".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: "a".repeat(40),
        source_sha: "b".repeat(40),
        plan_digest: digest_fill('8')?,
    })
}

fn fixture_context() -> Result<ExecutionContextV1, ContractError> {
    Ok(ExecutionContextV1 {
        schema: 1,
        job: fixture_job()?,
        generation: GenerationId {
            family: Gc1Family::State,
            group_digest: digest_fill('4')?,
            compatibility_digest: digest_fill('5')?,
            coverage_digest: digest_fill('6')?,
            repair_revision: 0,
        },
        requested_coverage_digest: digest_fill('7')?,
        source_binding_digest: digest_fill('9')?,
        workspace_root: "/home/runner/work/velnor-new/velnor-new".to_owned(),
        owned_root: "/home/runner/velnor-owned".to_owned(),
        cargo_home: "/home/runner/velnor-owned/cargo".to_owned(),
        rustup_home: "/home/runner/velnor-owned/rustup".to_owned(),
        tool_root: "/home/runner/velnor-owned/tools".to_owned(),
        transport_root: "/home/runner/work/velnor-new/transport".to_owned(),
        observation_root: "/home/runner/work/velnor-new/observations".to_owned(),
        lanes: vec![LaneContextV1 {
            lane_id: "lane-0".to_owned(),
            command_family: "build".to_owned(),
            variant_digest: digest_fill('a')?,
            target_dir: "target/debug".to_owned(),
            build_dir: None,
        }],
        compiler_adapters: vec![CompilerAdapterV1 {
            stable_path: "/opt/velnor/rustc".to_owned(),
            executable_identity: "rustc-1.99.1-x86_64".to_owned(),
        }],
        selected_driver: CacheDriver::Cargo,
        mbx_target_views: false,
        effective_cache_capability: CacheCapability::Read,
        trusted_default_writer: false,
        private_audience_digest: digest_fill('c')?,
    })
}

fn fixture_unknown() -> Result<MeasuredU64, ContractError> {
    let unknown = MeasuredU64 {
        value: None,
        unknown_reason: Some(ObservationGapV1 {
            code: ObservationGapCode::UnknownNativeCoverage,
            detail: "native calls outside compiler adapters".to_owned(),
            affected_recipe_ids: Vec::new(),
            evidence_ref: "observation-session-1".to_owned(),
        }),
    };
    unknown.validate()?;
    Ok(unknown)
}

fn fixture_lock_asset(id: &str) -> LockedAsset {
    LockedAsset {
        id: id.to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        tag: "v1.26.0".to_owned(),
        commit: "a".repeat(40),
        asset: "controller.tar.gz".to_owned(),
        sha256: "b".repeat(64),
        extract_to: None,
    }
}

fn fixture_lock() -> Result<DepsLock, ContractError> {
    let lock = DepsLock {
        schema: 1,
        maven_bundles: Vec::new(),
        release_assets: vec![fixture_lock_asset("tool-cache")],
        git_trees: Vec::new(),
    };
    lock.validate(".velnor/deps.lock")?;
    Ok(lock)
}

fn fixture_lock_config() -> PrivateSourcesConfig {
    PrivateSourcesConfig {
        owner: "tailrocks".to_owned(),
        app_id_variable: "APP_ID".to_owned(),
        app_private_key_secret: "APP_KEY".to_owned(),
        npm_scopes: Vec::new(),
        maven_bundles: Vec::new(),
        release_assets: vec![PrivateAssetSource {
            id: "tool-cache".to_owned(),
            repository: "tailrocks/velnor-new".to_owned(),
            tag: "v1.26.0".to_owned(),
            asset: "controller.tar.gz".to_owned(),
            extract_to: None,
        }],
        git_trees: Vec::new(),
    }
}

fn fixture_publication_identity() -> PublicationIdentity {
    PublicationIdentity {
        source_sha: "c".repeat(40),
        receipt_digest: "receipt-1".to_owned(),
        assets: BTreeMap::from([
            ("release.json".to_owned(), "digest-release".to_owned()),
            ("payload.tar.gz".to_owned(), "digest-payload".to_owned()),
        ]),
    }
}

fn publication_available(identity: &PublicationIdentity) -> BTreeSet<String> {
    identity.assets.keys().cloned().collect()
}

fn check_lock_identity() -> Result<(), Box<dyn Error>> {
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

fn check_feature_identity() -> Result<(), Box<dyn Error>> {
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

fn check_path_identity() -> Result<(), Box<dyn Error>> {
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

fn check_profile_identity() -> Result<(), Box<dyn Error>> {
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

fn check_toolchain_identity() -> Result<(), Box<dyn Error>> {
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

fn check_native_identity() -> Result<(), Box<dyn Error>> {
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

fn check_unknown_counters() -> Result<(), Box<dyn Error>> {
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

fn check_publication_decisions() {
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

#[test]
fn gc1_contract_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_lock_identity()?;
    check_feature_identity()?;
    check_path_identity()?;
    check_profile_identity()?;
    check_toolchain_identity()?;
    check_native_identity()?;
    check_unknown_counters()?;
    check_publication_decisions();
    Ok(())
}

fn rejects_malformed_coverage() -> Result<(), Box<dyn Error>> {
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

fn rejects_malformed_context() -> Result<(), Box<dyn Error>> {
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

fn rejects_malformed_publication() {
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

fn rejects_malformed_lock() -> Result<(), Box<dyn Error>> {
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

fn rejects_collapsed_counters() -> Result<(), Box<dyn Error>> {
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

#[test]
fn gc1_contract_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    rejects_malformed_coverage()?;
    rejects_malformed_context()?;
    rejects_malformed_publication();
    rejects_malformed_lock()?;
    rejects_collapsed_counters()?;
    Ok(())
}
