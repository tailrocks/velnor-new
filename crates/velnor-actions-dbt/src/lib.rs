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

pub const STACK_ID: &str = "dbt";

pub const PROJECT_FILE: &str = "dbt_project.yml";

pub const PACKAGES_FILE: &str = "packages.yml";

pub const PACKAGE_LOCK_FILE: &str = "package-lock.yml";

pub const PROFILES_FILE: &str = "profiles.yml";
