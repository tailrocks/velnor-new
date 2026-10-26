// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
pub mod bunfig;
pub mod error;
pub mod jsonc;
pub mod lockfile;
pub mod package_json;
pub mod paths;
pub mod playwright_script;
pub mod reads;
pub mod select;
pub mod tools;
pub mod workspace;

pub use bunfig::{NPM_REGISTRY_URL, NPM_TOKEN_ENV, velnor_bunfig};
pub use error::BunError;
pub use lockfile::{BUN_LOCK, check_js_lockfiles, parse_lock};
pub use package_json::{PackageJson, parse_package_json};
pub use playwright_script::check_shard_script;
pub use reads::{is_config_file, outside_reads};
pub use select::{affected_members, forced_members};
pub use tools::check_package_manager;
pub use workspace::{
    Member, PlaywrightCore, Workspace, job_id, job_name, member_job_scripts, member_scripts,
    resolve, resolve_member_dir, slug, suite_claims, suite_job_id, suite_job_name,
};

pub const STACK_ID: &str = "bun";
