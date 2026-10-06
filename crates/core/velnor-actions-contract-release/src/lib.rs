//! Release engineering vocabulary: manifests, targets, policy, verification records.
//!
//! Owns release and candidate manifests, release targets and the
//! runner-label catalog, tooling pins, stack-extension schemas, version
//! policy, freshness evidence, inspection findings, and declared output
//! formats. Must not own identities, configuration, detection, or
//! workflow IR. Built on `velnor-actions-contract` (identifiers).

pub mod candidate_manifest;
pub mod extensions;
pub mod finding;
pub mod formats;
pub mod freshness;
pub mod manifest;
pub(crate) mod manifest_checks;
pub mod policy;
pub mod targets;
pub mod tooling;

pub use extensions::{
    RUST_EXTENSION_REQUIRED_SLOTS, TOFU_EXTENSION_REQUIRED_SLOTS, validate_rust_extension,
    validate_tofu_extension,
};
pub use finding::Finding;
pub use formats::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, CLAUDE_MD_TARGET, DECLARED_GITHUB_FORMATS, find_github_format,
    is_declared_github_format,
};
pub use freshness::{
    FRESHNESS_CLASSES, FreshnessRequirement, RunnerImageEvidence, UNOBSERVED_IMAGE_VALUE,
    runner_family_changed, validate_freshness_class,
};
pub use manifest::{
    ActionPin, CandidateArtifactManifest, GeneratorBinary, GeneratorLock, LockedGenerator,
    MiseBootstrap, ReleaseManifest, TargetRecord, require_release_version,
};
pub use policy::{
    FreshnessEntry, FreshnessStatus, GithubRunnerImages, NightlyRecord, PolicyException,
    RunnerInventory, VersionPolicy, days_between,
};
pub use targets::{
    EXPECTED_REPOSITORY, LATEST_RUNNER_LABEL, RELEASE_MANIFEST_FILENAME, RUNNER_LABEL_CATALOG,
    ReleaseTarget, SUPPORTED_TARGETS, asset_filename, check_release_artifact,
    is_seed_tag_for_version, is_supported_target,
};
pub use tooling::ToolIdentity;
