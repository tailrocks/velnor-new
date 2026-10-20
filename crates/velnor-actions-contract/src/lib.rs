//! Stack-neutral workflow/task contracts.
//!
//! Owns task graphs, identities, reports, and recommendations. Must not own
//! Rust/Cargo, Mise, process, filesystem, YAML, CLI, or generic app models.
//!
//! All types here are effect-free data plus pure derivation/validation; derivation formulas are normative.

pub mod archive;
pub mod cachekey;
pub mod candidate_manifest;
pub mod canonical;
pub mod closure;
pub mod config;
pub mod deps_lock;
pub mod discover;
pub mod errors;
mod exports;
pub mod extension_schemas;
pub mod extensions;
pub mod finding;
pub mod formats;
pub mod freshness;
pub mod graph;
pub mod homebrew_formula;
pub mod homebrew_merge_gate;
pub mod homebrew_review_completion;
pub mod ids;
pub mod manifest;
pub(crate) mod manifest_checks;
pub mod marker;
pub mod package_release;
pub mod package_release_plan;
pub mod package_release_policy;
pub mod policy;
pub mod preview_identity;
pub mod preview_production;
pub mod propose;
pub mod publication_retry;
pub mod review_completion;
pub mod secrets;
pub mod strict_json;
pub mod targets;
pub mod tooling;
pub mod vcs;
pub mod workflow;
mod workflow_reexports;
pub use config::{
    DEFAULT_IMAGE_TIMEOUT_MINUTES, IMAGE_JOB_PREFIX, IMAGE_REGISTRY_HOST, ImageConfig,
    ImageDockerfile, ImagePlatform, ImagePrebuilt, PrivateAssetSource, PrivateGitTreeSource,
    PrivateSourcesConfig, ReleaseRegistry,
};
pub use exports::*;
