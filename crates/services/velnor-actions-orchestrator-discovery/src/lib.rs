//! Repository discovery: detection, inventory, profiles, recommendations.
//!
//! Second layer of the orchestrator family: turns a repository checkout
//! into [`discover::Discovery`] (workspaces, tofu units, profiles, tool
//! findings) plus the base/head edge and tofu-selection primitives that
//! selection and graph planning build on. Depends only on the core leaf.

pub mod derive_groups;
pub mod discover;
pub mod discover_index;
pub mod discover_tofu;
pub mod evidence;
pub mod inventory;
pub mod inventory_reuse;
pub mod recommendations;
pub mod select_edges;
pub mod select_tofu;
pub mod tool_snapshot;
pub mod toolcheck;
pub mod toolfindings;
