#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
pub mod collections;
pub mod config_file;
pub mod error;
pub mod locked;
pub mod paths;
pub mod playbooks;
pub mod requirements;
pub mod select;
pub mod vault;

pub use collections::{GALAXY_HOST, collection_url, parse_requirements_yaml, verify_collections};
pub use config_file::check_ansible_cfg;
pub use error::AnsibleError;
pub use locked::{LockedTool, check_locked_tool};
pub use playbooks::{is_playbook, top_level_playbooks};
pub use requirements::{Requirement, check_agreement, check_core_agreement, parse_requirements};
pub use select::affected_roots;
pub use vault::{VAULT_HEADER, check_vault};

pub const STACK_ID: &str = "ansible";

pub const CORE_DISTRIBUTION: &str = "ansible-core";

pub const LINT_DISTRIBUTION: &str = "ansible-lint";
