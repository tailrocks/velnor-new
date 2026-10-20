//! Ansible stack adapter: hash-pinned Python requirements, offline collection
//! pins, playbook discovery, vault refusal and affected-root claims.
//!
//! This crate only reads text the orchestrator hands it; it launches no
//! process, touches no file and renders no workflow text. Host rollout is out
//! of scope (hard decision 9): nothing here connects to a managed host.

pub mod collections;
pub mod error;
pub mod paths;
pub mod playbooks;
pub mod requirements;
pub mod select;
pub mod vault;

pub use collections::{GALAXY_HOST, collection_url, parse_requirements_yaml, verify_collections};
pub use error::AnsibleError;
pub use playbooks::{is_playbook, top_level_playbooks};
pub use requirements::{Requirement, check_core_agreement, parse_requirements};
pub use select::affected_roots;
pub use vault::{VAULT_HEADER, check_vault};

/// Stable identifier of the Ansible stack.
pub const STACK_ID: &str = "ansible";
/// Python distribution that provides `ansible-playbook`.
pub const CORE_DISTRIBUTION: &str = "ansible-core";
/// Python distribution that provides `ansible-lint`.
pub const LINT_DISTRIBUTION: &str = "ansible-lint";
