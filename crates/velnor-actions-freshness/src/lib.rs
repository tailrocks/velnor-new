#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod context;
mod dependencies;
mod evidence;
mod inventory;
mod members;
mod patterns;
mod probe;
mod process;
mod release_support;
mod time;
mod validation;

use std::path::{Path, PathBuf};

use crate::context::FreshnessContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Freshness {
        check_upstream: bool,

        with_advisories: bool,
    },

    ToolchainSpecs,

    MiseVersion,

    WorkspaceMembers {
        libraries_only: bool,
    },

    WorkspaceTestMembers {
        manifest_path: Option<PathBuf>,
    },

    CargoArtifactExecutable {
        messages_path: PathBuf,

        manifest_path: PathBuf,

        target_name: String,
    },
}

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
        Operation::WorkspaceTestMembers { manifest_path } => {
            members::print_test_members(root, manifest_path.as_deref())
        }
        Operation::CargoArtifactExecutable {
            messages_path,
            manifest_path,
            target_name,
        } => release_support::print_cargo_artifact_executable(
            root,
            messages_path,
            manifest_path,
            target_name,
        ),
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
