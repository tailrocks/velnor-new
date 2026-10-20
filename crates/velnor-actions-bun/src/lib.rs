//! Bun stack adapter: lockfile policy, workspace members, Velnor-owned
//! `bunfig.toml`, tool agreement and affected-member claims.
//!
//! Hard decision 6: one job per workspace member. This crate only reads text
//! the orchestrator hands it; it launches no process, touches no file and
//! renders no workflow text.

pub mod bunfig;
pub mod error;
pub mod jsonc;
pub mod lockfile;
pub mod package_json;
pub mod paths;
pub mod reads;
pub mod select;
pub mod tools;
pub mod workspace;

pub use bunfig::{NPM_REGISTRY_URL, NPM_TOKEN_ENV, velnor_bunfig};
pub use error::BunError;
pub use lockfile::{BUN_LOCK, check_js_lockfiles, parse_lock};
pub use package_json::{PackageJson, parse_package_json};
pub use reads::{is_config_file, outside_reads};
pub use select::{affected_members, forced_members};
pub use tools::check_package_manager;
pub use workspace::{
    Member, Workspace, job_id, job_name, member_scripts, resolve, resolve_member_dir, slug,
};

/// Stable identifier of the Bun stack.
pub const STACK_ID: &str = "bun";
