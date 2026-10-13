//! Velnor repository-owned version and dependency policy checks.
//!
//! This maintenance gate is intentionally separate from the V1 workflow
//! generator. It reads reviewed inventories and source metadata, and emits
//! bounded machine-readable evidence for local and scheduled checks.

mod context;
mod dependencies;
mod evidence;
mod inventory;
mod members;
mod patterns;
mod probe;
mod process;
mod time;
mod trailer_policy;
mod validation;

use std::path::{Path, PathBuf};

use crate::context::FreshnessContext;

/// Typed operation transported through the existing Velnor CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Run the freshness, lock identity, and offline deny-policy gates.
    Freshness {
        /// Probe current upstream releases using bounded read-only requests.
        check_upstream: bool,
        /// Run live cargo-deny advisory scans for all discovered workspaces.
        with_advisories: bool,
    },
    /// Print pinned `mise exec` specs after comparing Mise and policy values.
    ToolchainSpecs,
    /// Print the pinned Mise version from version policy.
    MiseVersion,
    /// Print sorted Cargo package names.
    WorkspaceMembers {
        /// Include packages with a library target only.
        libraries_only: bool,
    },
    /// Validate one commit message against repository trailer policy.
    TrailerPolicy {
        /// Absolute path to the message being validated.
        message_path: PathBuf,
        /// Check local author and committer identity using Git.
        check_local_identities: bool,
    },
}

/// Run one repository policy operation under `root`.
#[must_use]
pub fn run(root: &Path, operation: &Operation) -> i32 {
    match operation {
        Operation::Freshness {
            check_upstream,
            with_advisories,
        } => run_freshness(root.to_path_buf(), *check_upstream, *with_advisories),
        Operation::ToolchainSpecs => print_toolchain_specs(root),
        Operation::MiseVersion => print_mise_version(root),
        Operation::WorkspaceMembers { libraries_only } => {
            members::print_workspace_members(root, *libraries_only)
        }
        Operation::TrailerPolicy {
            message_path,
            check_local_identities,
        } => trailer_policy::run(root, message_path, *check_local_identities),
    }
}

fn run_freshness(root: PathBuf, check_upstream: bool, with_advisories: bool) -> i32 {
    let mut ctx = FreshnessContext::new(root, check_upstream, with_advisories);
    if !inventory::load_inventory(&mut ctx) {
        return ctx.finish();
    }
    inventory::load_policy(&mut ctx);
    inventory::check_policy_header(&mut ctx);
    inventory::check_local_pins(&mut ctx);
    inventory::check_policy_mirror(&mut ctx);
    validation::check_validation_tool_pins(&mut ctx);
    dependencies::check_effective_identity(&mut ctx);
    evidence::check_recorded_freshness(&mut ctx);
    evidence::check_exceptions(&mut ctx);
    evidence::check_advisories(&mut ctx);
    if ctx.check_upstream {
        probe::check_upstream_probe(&mut ctx);
    }
    ctx.finish()
}

fn print_toolchain_specs(root: &Path) -> i32 {
    match inventory::toolchain_specs(root) {
        Ok(specs) => {
            println!("{}", specs.join(" "));
            0
        }
        Err(error) => {
            eprintln!("repository policy: {error}");
            1
        }
    }
}

fn print_mise_version(root: &Path) -> i32 {
    match inventory::policy_mise_version(root) {
        Ok(version) => {
            println!("{version}");
            0
        }
        Err(error) => {
            eprintln!("repository policy: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests;
