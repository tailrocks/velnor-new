//! Native desktop delivery ownership and closed qualification boundary.
//!
//! Typed topology is staged in `desktop_delivery_graph.rs` until Rust producers,
//! Swift consumers, source registry, and protected publishing are qualified.

#[path = "desktop_delivery_files.rs"]
mod files;
#[path = "desktop_delivery_tools.rs"]
mod tools;

pub use files::desktop_helper_files;
pub use tools::{NativeCompileDriver, NativeDesktopToolContext};

/// Native desktop generation stays closed until complete owner qualification.
pub(crate) fn require_qualified_profile()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    Err(
        velnor_actions_workflow_renderer::RenderError::InvalidWorkflow(
            "desktop_profile_unqualified".to_owned(),
        ),
    )
}

// Topology and operation-shape proof only; this grants no execution authority.
#[cfg(test)]
#[path = "desktop_delivery_graph.rs"]
mod graph;
