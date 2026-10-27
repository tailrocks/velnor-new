use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::canonical::{Digest, digest_b3_typed};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::workflow::cache_identity::{CompatibilityV1, Gc1Family, GenerationId};
use velnor_actions_contract::workflow::cache_policy::{
    ALLOWED_UNIVERSAL_KEY_FIELDS, CompletedCoverageV1, FORBIDDEN_UNIVERSAL_KEY_FIELDS,
    ImmutableObjectKind, ImmutableObjectV1, RecipeV1, RequestedCoverageV1,
    validate_universal_key_field,
};

fn digest_fill(fill: char) -> Result<Digest, ContractError> {
    Digest::parse(&format!("b3-{}", fill.to_string().repeat(64)))
}

fn fixture_compatibility(family: Gc1Family) -> Result<CompatibilityV1, ContractError> {
    Ok(CompatibilityV1 {
        schema: 1,
        family,
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

fn fixture_object(object_id: &str, fill: char) -> Result<ImmutableObjectV1, ContractError> {
    let content = digest_fill(fill)?;
    Ok(ImmutableObjectV1 {
        object_id: object_id.to_owned(),
        kind: ImmutableObjectKind::Source,
        approved_origin_id: "crates-io".to_owned(),
        immutable_locator: format!("sparse-index/{}", content.as_str()),
        content_digest: content,
        audience_digest: digest_fill('9')?,
    })
}

fn fixture_requested() -> Result<RequestedCoverageV1, ContractError> {
    let compatibility = fixture_compatibility(Gc1Family::State)?;
    let first = fixture_recipe("unit-a")?;
    let second = fixture_recipe("unit-b")?;
    let mut recipes = vec![first, second];
    recipes.sort_by(|left, right| left.recipe_id.as_str().cmp(right.recipe_id.as_str()));
    let ordered_recipe_ids: Vec<String> = recipes
        .iter()
        .map(|recipe| recipe.recipe_id.as_str().to_owned())
        .collect();
    let mut coverage = RequestedCoverageV1 {
        schema: 1,
        generation: GenerationId {
            family: Gc1Family::State,
            group_digest: digest_fill('4')?,
            compatibility_digest: digest_fill('5')?,
            coverage_digest: digest_fill('6')?,
            repair_revision: 0,
        },
        compatibility,
        ordered_recipe_ids,
        recipes,
        immutable_objects: vec![fixture_object("registry-index", '7')?],
        active_variant_ids: vec!["default".to_owned()],
        dependency_inputs_digest: digest_fill('8')?,
    };
    let coverage_digest = coverage.coverage_digest()?;
    let compatibility_digest = coverage.compatibility.digest()?;
    coverage.generation.coverage_digest = coverage_digest;
    coverage.generation.compatibility_digest = compatibility_digest;
    coverage.validate()?;
    Ok(coverage)
}

fn fixture_completed(
    requested: &RequestedCoverageV1,
    ids: Vec<String>,
) -> Result<CompletedCoverageV1, ContractError> {
    Ok(CompletedCoverageV1 {
        schema: 1,
        requested_coverage_digest: digest_b3_typed(&requested.to_canonical_bytes()?),
        compatibility_digest: requested.compatibility.digest()?,
        completed_recipe_ids: ids,
    })
}

#[test]
fn gc1_t04_preserves_contract() -> Result<(), Box<dyn Error>> {
    let requested = fixture_requested()?;
    let bytes = requested.to_canonical_bytes()?;
    let text = String::from_utf8(bytes.clone())?;
    let roundtrip = RequestedCoverageV1::parse_json(&text)?;
    assert_eq!(requested, roundtrip);
    assert_eq!(requested.coverage_digest()?, roundtrip.coverage_digest()?);

    let rebuilt = fixture_requested()?;
    assert_eq!(requested.coverage_digest()?, rebuilt.coverage_digest()?);
    assert_eq!(
        requested.compatibility.digest()?,
        rebuilt.compatibility.digest()?
    );

    let digest = requested.compatibility.digest()?;
    let mut relabeled = requested.compatibility.clone();
    relabeled.runner_image_identity = "ubuntu-24.04-20261101".to_owned();
    assert_eq!(digest, relabeled.digest()?);
    relabeled.toolchain_identity = "stable-1.99.2".to_owned();
    assert_ne!(digest, relabeled.digest()?);

    let key = requested.generation.cache_key();
    assert!(key.starts_with("velnor-gc1-state-b3-"));
    assert!(key.ends_with("-r0"));
    let parsed = GenerationId::parse_key(&key)?;
    assert_eq!(parsed, requested.generation);
    assert_eq!(parsed.cache_key(), key);
    GenerationId::validate_key(&key)?;

    let completed = fixture_completed(&requested, requested.ordered_recipe_ids.clone())?;
    completed.validate()?;
    assert!(completed.covers(&requested)?);
    let first_id = requested.ordered_recipe_ids[0].clone();
    let partial = fixture_completed(&requested, vec![first_id])?;
    assert!(partial.covers(&requested)?);
    assert_eq!(
        completed.completed_coverage_digest()?,
        fixture_completed(&requested, requested.ordered_recipe_ids.clone())?
            .completed_coverage_digest()?
    );
    Ok(())
}

fn rejects_bad_compatibility() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_compatibility(Gc1Family::Tools)?;
    bad.schema = 2;
    assert!(bad.validate().is_err());
    let mut bad = fixture_compatibility(Gc1Family::Tools)?;
    bad.repository_id = 0;
    assert!(bad.validate().is_err());
    let mut bad = fixture_compatibility(Gc1Family::Tools)?;
    bad.host_os.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_compatibility(Gc1Family::Tools)?;
    bad.target_environment_ids = vec!["b".to_owned(), "a".to_owned()];
    assert!(bad.validate().is_err());
    assert!(Digest::parse("b3-xyz").is_err());
    assert!(Digest::parse("s256-aaaa").is_err());
    let record = fixture_compatibility(Gc1Family::Sources)?;
    let text = String::from_utf8(record.to_canonical_bytes()?)?;
    assert!(CompatibilityV1::parse_json("not json").is_err());
    let tampered = text.replacen("\"schema\":1", "\"schema\":1,\"surprise\":true", 1);
    assert!(CompatibilityV1::parse_json(&tampered).is_err());
    Ok(())
}

fn rejects_bad_recipe() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_recipe("unit-a")?;
    bad.recipe_id = digest_fill('4')?;
    assert!(bad.validate().is_err());
    let mut bad = fixture_recipe("unit-a")?;
    bad.argv.clear();
    assert!(bad.validate().is_err());
    let mut bad = fixture_recipe("unit-a")?;
    "/tmp".clone_into(&mut bad.cwd_relative);
    assert!(bad.validate().is_err());
    let mut bad = fixture_recipe("unit-a")?;
    bad.features = vec!["z".to_owned(), "a".to_owned()];
    assert!(bad.validate().is_err());
    let mut bad = fixture_recipe("unit-a")?;
    bad.declared_nonsecret_environment
        .insert("GITHUB_TOKEN".to_owned(), "secret".to_owned());
    assert!(bad.validate().is_err());
    let mut bad = fixture_object("registry-index", '7')?;
    "https://example.com/tool.tar.gz".clone_into(&mut bad.immutable_locator);
    assert!(bad.validate().is_err());
    Ok(())
}

fn rejects_bad_coverage() -> Result<(), Box<dyn Error>> {
    let mut bad = fixture_requested()?;
    bad.recipes.push(bad.recipes[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = fixture_requested()?;
    bad.ordered_recipe_ids.reverse();
    assert!(bad.validate().is_err());
    let mut bad = fixture_requested()?;
    bad.generation.family = Gc1Family::Tools;
    assert!(bad.validate().is_err());
    let mut bad = fixture_requested()?;
    bad.generation.coverage_digest = digest_fill('5')?;
    assert!(bad.validate().is_err());
    let mut bad = fixture_requested()?;
    bad.immutable_objects
        .push(fixture_object("registry-index", '6')?);
    assert!(bad.validate().is_err());
    let empty = fixture_completed(&fixture_requested()?, Vec::new())?;
    assert!(empty.validate().is_err());
    let requested = fixture_requested()?;
    let mut reversed = requested.ordered_recipe_ids.clone();
    reversed.reverse();
    assert!(fixture_completed(&requested, reversed)?.validate().is_err());
    let mut foreign = fixture_completed(&requested, requested.ordered_recipe_ids.clone())?;
    foreign.requested_coverage_digest = digest_fill('5')?;
    assert!(!foreign.covers(&requested)?);
    assert!(RequestedCoverageV1::parse_json("[]").is_err());
    Ok(())
}

fn rejects_volatile_key_inputs() -> Result<(), Box<dyn Error>> {
    for field in FORBIDDEN_UNIVERSAL_KEY_FIELDS {
        assert!(validate_universal_key_field(field).is_err());
    }
    for field in ALLOWED_UNIVERSAL_KEY_FIELDS {
        validate_universal_key_field(field)?;
    }
    assert!(validate_universal_key_field("").is_err());
    Ok(())
}

fn rejects_bad_keys() -> Result<(), Box<dyn Error>> {
    let requested = fixture_requested()?;
    let key = requested.generation.cache_key();
    let bad_namespace = key.replacen("velnor-gc1-", "velnor-gc0-", 1);
    assert!(GenerationId::parse_key(&bad_namespace).is_err());
    let bad_family = key.replacen("-state-", "-caches-", 1);
    assert!(GenerationId::parse_key(&bad_family).is_err());
    let bad_repair = key.replacen("-r0", "-rx", 1);
    assert!(GenerationId::parse_key(&bad_repair).is_err());
    let truncated = key[..key.len() - 4].to_owned();
    assert!(GenerationId::parse_key(&truncated).is_err());
    let oversized = format!("velnor-gc1-{}", "x".repeat(600));
    assert!(GenerationId::parse_key(&oversized).is_err());
    Ok(())
}

#[test]
fn gc1_t04_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    rejects_bad_compatibility()?;
    rejects_bad_recipe()?;
    rejects_bad_coverage()?;
    rejects_volatile_key_inputs()?;
    rejects_bad_keys()?;
    Ok(())
}
