//! Bounded, source-authenticated Cargo manifest projection for binary releases.
//!
//! This is intentionally not a replacement for Cargo metadata. It projects
//! only the selected workspace and configured binary products needed by the
//! release contract, using exact-source TOML blobs and a caller-authenticated
//! inventory of regular paths. It never runs Cargo or reads the filesystem.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ContractError,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProduct},
};

use crate::metadata::{PackageRecord, TargetRecord, VelnorV1TaskOwner, WorkspaceRecord};

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_MANIFESTS: usize = 4096;
const MAX_TOTAL_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_REGULAR_PATHS: usize = 250_000;
const MAX_GLOB_STEPS: usize = 16_000_000;
const MAX_GLOB_PATTERNS: usize = 128;
const MAX_REPO_PATH_BYTES: usize = 1024;
const MAX_GLOB_PATTERN_BYTES: usize = 1024;

struct ParsedReleaseManifests<'a> {
    values: BTreeMap<&'a str, toml::Value>,
    names: BTreeMap<String, String>,
}

#[path = "release_projection_members.rs"]
mod release_projection_members;
#[path = "release_projection_products.rs"]
mod release_projection_products;
#[path = "release_projection_shared.rs"]
mod release_projection_shared;

pub use release_projection_members::release_workspace_manifest_paths;
pub use release_projection_products::project_release_workspace;
use release_projection_shared::{
    glob_matches, has_glob_metacharacters, invalid, join_path, normalize_directory, normalize_path,
    parent, parse_manifest, read_string_array, relative_to, string_array, valid_package_name,
    validate_pattern, validate_repo_path,
};
