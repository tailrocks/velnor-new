// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::config::{PrivateAssetSource, PrivateSourcesConfig};
use velnor_actions_contract::deps_lock::{DepsLock, LockedAsset};
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::publication_retry::PublicationIdentity;
use velnor_actions_contract::workflow::cache_context::{
    CacheCapability, CacheDriver, CompilerAdapterV1, ExecutionContextV1, JobBindingV1,
    LaneContextV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{CompatibilityV1, Gc1Family, GenerationId};
use velnor_actions_contract::workflow::cache_policy::{
    ImmutableObjectKind, ImmutableObjectV1, RecipeV1, RequestedCoverageV1,
};

pub(super) fn digest_fill(fill: char) -> Result<Digest, ContractError> {
    Digest::parse(&format!("b3-{}", fill.to_string().repeat(64)))
}

pub(super) fn fixture_compatibility() -> Result<CompatibilityV1, ContractError> {
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

pub(super) fn fixture_recipe(unit_id: &str) -> Result<RecipeV1, ContractError> {
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

pub(super) fn retune_recipe(mut recipe: RecipeV1) -> Result<RecipeV1, ContractError> {
    recipe.recipe_id = recipe.computed_recipe_id()?;
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn fixture_object(object_id: &str) -> Result<ImmutableObjectV1, ContractError> {
    Ok(ImmutableObjectV1 {
        object_id: object_id.to_owned(),
        kind: ImmutableObjectKind::Source,
        approved_origin_id: "crates-io".to_owned(),
        immutable_locator: format!("sparse-index/{}", digest_fill('7')?.as_str()),
        content_digest: digest_fill('7')?,
        audience_digest: digest_fill('9')?,
    })
}

pub(super) fn rebuild_coverage(
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

pub(super) fn fixture_coverage() -> Result<RequestedCoverageV1, ContractError> {
    rebuild_coverage(
        fixture_compatibility()?,
        vec![fixture_recipe("unit-a")?, fixture_recipe("unit-b")?],
        digest_fill('8')?,
    )
}

pub(super) fn fixture_job() -> Result<JobBindingV1, ContractError> {
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

pub(super) fn fixture_context() -> Result<ExecutionContextV1, ContractError> {
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

pub(super) fn fixture_unknown() -> Result<MeasuredU64, ContractError> {
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

pub(super) fn fixture_lock_asset(id: &str) -> LockedAsset {
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

pub(super) fn fixture_lock() -> Result<DepsLock, ContractError> {
    let lock = DepsLock {
        schema: 1,
        maven_bundles: Vec::new(),
        release_assets: vec![fixture_lock_asset("tool-cache")],
        git_trees: Vec::new(),
    };
    lock.validate(".velnor/deps.lock")?;
    Ok(lock)
}

pub(super) fn fixture_lock_config() -> PrivateSourcesConfig {
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

pub(super) fn fixture_publication_identity() -> PublicationIdentity {
    PublicationIdentity {
        source_sha: "c".repeat(40),
        receipt_digest: "receipt-1".to_owned(),
        assets: BTreeMap::from([
            ("release.json".to_owned(), "digest-release".to_owned()),
            ("payload.tar.gz".to_owned(), "digest-payload".to_owned()),
        ]),
    }
}

pub(super) fn publication_available(identity: &PublicationIdentity) -> BTreeSet<String> {
    identity.assets.keys().cloned().collect()
}
