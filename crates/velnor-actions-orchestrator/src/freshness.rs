//! Native repository freshness gate used by the shell entrypoint.

mod advisories;
mod context;
mod dependencies;
mod dependency_graph;
mod evidence;
mod exceptions;
mod glob;
mod inventory;
mod local_pins;
mod lock_validation;
mod policy_mirror;
mod probe;
mod rust_source;
mod validation;

#[cfg(test)]
#[path = "freshness/context_offset_tests.rs"]
mod context_offset_tests;

#[cfg(test)]
#[path = "rust_source_tests.rs"]
mod rust_source_tests;

use std::path::Path;

use context::FreshnessContext;

/// Run the repository freshness checks in their documented order.
///
/// # Errors
///
/// Returns an error if writing check output fails.
pub fn check_freshness(
    root: &Path,
    check_upstream: bool,
    with_advisories: bool,
) -> Result<i32, std::io::Error> {
    let mut ctx = FreshnessContext::new(root, check_upstream, with_advisories);
    if !inventory::load_inventory(&mut ctx) {
        return ctx.finish();
    }
    inventory::load_policy(&mut ctx);
    inventory::check_policy_header(&mut ctx);
    local_pins::check_local_pins(&mut ctx);
    policy_mirror::check_policy_mirror(&mut ctx);
    validation::check_validation_tool_pins(&mut ctx);
    dependencies::check_effective_identity(&mut ctx);
    evidence::check_recorded_freshness(&mut ctx);
    exceptions::check_exceptions(&mut ctx);
    advisories::check_advisories(&mut ctx);
    if ctx.check_upstream {
        probe::check_upstream_probe(&mut ctx);
    }
    ctx.finish()
}
