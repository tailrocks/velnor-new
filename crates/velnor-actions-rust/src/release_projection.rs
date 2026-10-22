//! Bounded, source-authenticated Cargo manifest projection for binary releases.
//!
//! This is intentionally not a replacement for Cargo metadata. It projects
//! only the selected workspace and configured binary products needed by the
//! release contract, using exact-source TOML blobs and a caller-authenticated
//! inventory of regular paths. It never runs Cargo or reads the filesystem.

mod product_targets;
mod project;
mod source;
mod workspace;

pub use project::project_release_workspace;
pub use workspace::release_workspace_manifest_paths;
