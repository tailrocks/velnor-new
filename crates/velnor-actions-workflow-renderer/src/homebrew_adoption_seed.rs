// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Serialize;
use velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition;

use crate::{
    RenderError,
    render::RenderedFile,
    review_completion::{ReviewCompletionSpec, render_review_completion},
    schema2::ProductReleasePins,
    yaml::Yaml,
};
use velnor_actions_contract::{
    ReleaseTarget,
    config::{ReviewCompletionConfig, ReviewCompletionProvider},
    review_completion::{REVIEW_COMPLETION_POLICY_SCHEMA, ReviewCompletionPolicy},
};

#[path = "homebrew_adoption_seed_identity.rs"]
mod identity;
use identity::{lower_hex, sha256, version_at_least};

const SDK_REPOSITORY: &str = "tailrocks/velnor-new";
const SDK_TARGET: &str = "x86_64-unknown-linux-gnu";
const REUSABLE_WORKFLOW: &str = ".github/workflows/homebrew-adoption.yml";
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewAdoptionSeedSdkIdentity<'a> {
    pub source_sha: &'a str,
    pub version: &'a str,
    pub manifest_bytes: &'a [u8],
    pub manifest_sha256: &'a str,
    pub host_target_sha256: &'a str,
}

pub fn render_homebrew_adoption_seed(
    transition: &HomebrewAdoptionTransition,
    sdk: &HomebrewAdoptionSeedSdkIdentity<'_>,
    pins: &ProductReleasePins,
) -> Result<Vec<RenderedFile>, RenderError> {
    transition
        .validate()
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    if transition.template_version != 3
        || transition.required_sdk_capability != "homebrew-adoption-seed-v1"
        || !transition
            .generated_output_paths
            .iter()
            .map(String::as_str)
            .eq(
                velnor_actions_contract::homebrew_adoption::HOMEBREW_ADOPTION_SEED_V3_OUTPUT_PATHS
                    .iter()
                    .copied(),
            )
    {
        return Err(seed_error("unsupported_transition_template"));
    }
    validate_sdk_identity(transition, sdk)?;
    let callee =
        crate::homebrew_adoption_workflow::render_homebrew_adoption_callee(sdk.version, pins)?;
    let callee_sha256 = sha256(callee.file.bytes.as_bytes());
    let scope = crate::workflow_timeout::HomebrewAdoptionCalleeScope::authenticated_seed(
        sdk.source_sha,
        &transition.transition_id,
        sdk.version,
        sdk.manifest_sha256,
        &callee_sha256,
    );
    let ci = render_caller_workflow(transition, sdk, &scope)?;
    let config = render_config(transition)?;
    let review_completion = render_review_completion_seed(transition, sdk, pins)?;
    let mise = render_mise_config()?;
    let lock = render_mise_lock()?;
    let files = vec![
        RenderedFile {
            path: ".github/workflows/ci.yml".to_owned(),
            bytes: ci,
        },
        RenderedFile {
            path: ".github/workflows/review-completion.yml".to_owned(),
            bytes: review_completion,
        },
        RenderedFile {
            path: ".velnor/config.toml".to_owned(),
            bytes: config,
        },
        RenderedFile {
            path: ".velnor/release-manifest.json".to_owned(),
            bytes: String::from_utf8(sdk.manifest_bytes.to_vec())
                .map_err(|_| seed_error("sdk_manifest_utf8"))?,
        },
        RenderedFile {
            path: "mise.lock".to_owned(),
            bytes: lock,
        },
        RenderedFile {
            path: "mise.toml".to_owned(),
            bytes: mise,
        },
    ];
    let expected: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();
    if !expected
        .iter()
        .copied()
        .eq(transition.generated_output_paths.iter().map(String::as_str))
    {
        return Err(seed_error("renderer_output_inventory_mismatch"));
    }
    Ok(files)
}

fn render_review_completion_seed(
    transition: &HomebrewAdoptionTransition,
    sdk: &HomebrewAdoptionSeedSdkIdentity<'_>,
    pins: &ProductReleasePins,
) -> Result<String, RenderError> {
    let config = review_completion_config();
    let manifest =
        std::str::from_utf8(sdk.manifest_bytes).map_err(|_| seed_error("sdk_manifest_utf8"))?;
    let acquire = crate::native_helper::consumer_acquire_for_target(
        ReleaseTarget::LinuxX86_64,
        sdk.version,
        Some(manifest),
    )
    .map_err(|error| match error {
        crate::native_helper::ReleaseHelperError::Contract(error) => RenderError::Contract(error),
        crate::native_helper::ReleaseHelperError::Render(error) => error,
        _ => seed_error("review_completion_acquisition"),
    })?;
    let policy = ReviewCompletionPolicy {
        schema: REVIEW_COMPLETION_POLICY_SCHEMA.to_owned(),
        repository: transition.tap_repository.clone(),
        default_branch: "main".to_owned(),
        provider: config.provider,
        check_name: config.check_name.clone(),
    };
    let rendered = render_review_completion(&ReviewCompletionSpec {
        policy,
        config,
        generator_version: sdk.version.to_owned(),
        setup: pins.linux_x86_64_setup.clone(),
        acquire,
        install_gh_argv: pins.install_gh_argv.clone(),
    })?;
    Ok(rendered.bytes)
}

fn review_completion_config() -> ReviewCompletionConfig {
    ReviewCompletionConfig {
        provider: ReviewCompletionProvider::CodexV1,
        check_name: "Review completion".to_owned(),
        app_id_variable: "REVIEW_COMPLETION_APP_ID".to_owned(),
        app_private_key_secret: "REVIEW_COMPLETION_APP_PRIVATE_KEY".to_owned(),
        reviewer_app_id_variable: "REVIEWER_APP_ID".to_owned(),
    }
}

fn validate_sdk_identity(
    transition: &HomebrewAdoptionTransition,
    sdk: &HomebrewAdoptionSeedSdkIdentity<'_>,
) -> Result<(), RenderError> {
    if sdk.manifest_bytes.is_empty() || sdk.manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(seed_error("sdk_manifest_size"));
    }
    if !lower_hex(sdk.source_sha, 40)
        || !lower_hex(sdk.manifest_sha256, 64)
        || !lower_hex(sdk.host_target_sha256, 64)
        || sha256(sdk.manifest_bytes) != sdk.manifest_sha256
        || !version_at_least(sdk.version, &transition.minimum_sdk_version)
    {
        return Err(seed_error("sdk_binding_invalid"));
    }
    let manifest_text =
        std::str::from_utf8(sdk.manifest_bytes).map_err(|_| seed_error("sdk_manifest_utf8"))?;
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json_with_limit(
        manifest_text,
        "release-manifest.json",
        MAX_MANIFEST_BYTES,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    manifest
        .validate("release-manifest.json")
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    if manifest.repository != SDK_REPOSITORY
        || manifest.commit != sdk.source_sha
        || manifest.version != sdk.version
    {
        return Err(seed_error("sdk_manifest_identity_mismatch"));
    }
    let target = manifest
        .record_for_target(SDK_TARGET)
        .ok_or_else(|| seed_error("sdk_manifest_target_missing"))?;
    if target.sha256 != sdk.host_target_sha256 {
        return Err(seed_error("sdk_manifest_target_digest_mismatch"));
    }
    Ok(())
}

fn render_caller_workflow(
    transition: &HomebrewAdoptionTransition,
    sdk: &HomebrewAdoptionSeedSdkIdentity<'_>,
    scope: &crate::workflow_timeout::HomebrewAdoptionCalleeScope,
) -> Result<String, RenderError> {
    let reference = format!("{SDK_REPOSITORY}/{REUSABLE_WORKFLOW}@{}", sdk.source_sha);
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("CI")),
        (
            "on".to_owned(),
            crate::workflow_timeout::pull_request_only(),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("contents".to_owned(), Yaml::str("read")),
                ("actions".to_owned(), Yaml::str("read")),
                ("attestations".to_owned(), Yaml::str("read")),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "homebrew-adoption".to_owned(),
                Yaml::Map(vec![
                    ("uses".to_owned(), Yaml::str(reference)),
                    (
                        "with".to_owned(),
                        Yaml::Map(vec![
                            (
                                "transition_id".to_owned(),
                                Yaml::str(transition.transition_id.clone()),
                            ),
                            ("sdk_version".to_owned(), Yaml::str(sdk.version)),
                            ("sdk_source_sha".to_owned(), Yaml::str(sdk.source_sha)),
                            (
                                "sdk_manifest_sha256".to_owned(),
                                Yaml::str(sdk.manifest_sha256),
                            ),
                        ]),
                    ),
                    (
                        "permissions".to_owned(),
                        Yaml::Map(vec![
                            ("contents".to_owned(), Yaml::str("read")),
                            ("actions".to_owned(), Yaml::str("read")),
                            ("attestations".to_owned(), Yaml::str("read")),
                        ]),
                    ),
                ]),
            )]),
        ),
    ]);
    crate::render::fallback::render_checked_homebrew_adoption_caller(
        ".github/workflows/ci.yml",
        &document,
        sdk.version,
        scope,
    )
}

fn render_config(transition: &HomebrewAdoptionTransition) -> Result<String, RenderError> {
    let config = SeedConfig {
        schema: 1,
        workflow: SeedWorkflow {
            name: "CI",
            policy: "consumer-v1",
            default_branch: "main",
            generator_validation: "bootstrap",
            max_parallel_jobs: 2,
            review_completion: review_completion_config(),
        },
        resources: SeedResources {
            compiler_process_budget: 2,
            test_process_budget: 2,
        },
        test_sharding: SeedTestSharding { default_shards: 1 },
        stacks: SeedStacks { ignore: Vec::new() },
        discovery: SeedDiscovery {
            exclude: Vec::new(),
        },
        native_checks: transition
            .required_operations
            .iter()
            .map(|operation| SeedNativeCheck {
                id: operation.as_str(),
                operation: operation.as_str(),
                runner: SeedRunner {
                    label: "ubuntu-26.04",
                    platform: "linux_x64",
                    executor: "hosted",
                },
                timeout_minutes: 5,
            })
            .collect(),
    };
    toml::to_string(&config).map_err(|_| seed_error("seed_config_serialization"))
}

fn render_mise_config() -> Result<String, RenderError> {
    let config = SeedMiseConfig {
        settings: SeedMiseSettings { lockfile: true },
        tools: BTreeMap::<String, String>::new(),
    };
    toml::to_string(&config).map_err(|_| seed_error("seed_mise_serialization"))
}

fn render_mise_lock() -> Result<String, RenderError> {
    toml::to_string(&SeedMiseLock {
        lockfile_version: 3,
    })
    .map_err(|_| seed_error("seed_lock_serialization"))
}

#[derive(Serialize)]
struct SeedConfig {
    schema: u32,
    workflow: SeedWorkflow,
    resources: SeedResources,
    test_sharding: SeedTestSharding,
    stacks: SeedStacks,
    discovery: SeedDiscovery,
    native_checks: Vec<SeedNativeCheck>,
}

#[derive(Serialize)]
struct SeedWorkflow {
    name: &'static str,
    policy: &'static str,
    default_branch: &'static str,
    generator_validation: &'static str,
    max_parallel_jobs: u32,
    review_completion: ReviewCompletionConfig,
}

#[derive(Serialize)]
struct SeedResources {
    compiler_process_budget: u32,
    test_process_budget: u32,
}

#[derive(Serialize)]
struct SeedTestSharding {
    default_shards: u32,
}

#[derive(Serialize)]
struct SeedStacks {
    ignore: Vec<&'static str>,
}

#[derive(Serialize)]
struct SeedDiscovery {
    exclude: Vec<&'static str>,
}

#[derive(Serialize)]
struct SeedNativeCheck {
    id: &'static str,
    operation: &'static str,
    runner: SeedRunner,
    timeout_minutes: u32,
}

#[derive(Serialize)]
struct SeedRunner {
    label: &'static str,
    platform: &'static str,
    executor: &'static str,
}

#[derive(Serialize)]
struct SeedMiseConfig {
    settings: SeedMiseSettings,
    tools: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct SeedMiseSettings {
    lockfile: bool,
}

#[derive(Serialize)]
struct SeedMiseLock {
    lockfile_version: u32,
}

fn seed_error(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("homebrew_adoption_seed:{problem}"))
}
