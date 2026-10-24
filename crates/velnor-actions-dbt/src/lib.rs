//! Check dbt-fusion versions, project data, package locks, profiles, and artifact records.
//! Select projects affected by source changes.
//!
//! This crate reads text from the orchestrator. It does not start processes,
//! access files, or render workflow text.

pub mod error;
pub mod packages;
pub mod paths;
pub mod profiles;
pub mod project;
pub mod select;
pub mod tools;
pub mod version;
pub mod yaml;

pub use error::DbtError;
pub use packages::{Package, PackageSource, parse_packages, verify_lock};
pub use profiles::{check_profiles, uses_postgres_adapter};
pub use project::{Project, ProjectFiles, analyze_project, job_id, job_name, slug};
pub use select::affected_projects;
pub use tools::{FUSION_HOST, FusionPlatform, check_mise_lock, fusion_url};
pub use version::{FusionVersion, VersionError, parse_fusion_version};

/// Stable identifier of the dbt stack.
pub const STACK_ID: &str = "dbt";
/// File that marks a dbt project root.
pub const PROJECT_FILE: &str = "dbt_project.yml";
/// Package manifest next to the project file.
pub const PACKAGES_FILE: &str = "packages.yml";
/// Resolved package lock next to the manifest.
pub const PACKAGE_LOCK_FILE: &str = "package-lock.yml";
/// Connection profile file.
pub const PROFILES_FILE: &str = "profiles.yml";
