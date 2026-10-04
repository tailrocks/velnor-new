//! Pure preparation recipes and cold admission, separate from catalog selection.

use super::{PinnedTool, ToolCatalog, homebrew, qualification, rust_prepare, tool_prepare};

/// Anonymous admission of restored Rust execution state.
#[path = "catalog_rust_cold.rs"]
pub mod rust_cold;

/// Neutral tool recipes for native validation factories.
#[path = "catalog_native_validation.rs"]
pub mod native_validation;

/// Source and validation desktop tool recipes.
#[path = "catalog_native_desktop.rs"]
pub mod native_desktop;

/// Host-qualified delivery tool recipes and their pure argv prefix.
#[path = "catalog_delivery_tools.rs"]
pub mod delivery_tools;

/// Source-qualified locations and environment for selected native tools.
#[path = "catalog_native_tool_context.rs"]
pub mod native_tool_context;

/// Captured native preparation recipes for same-process receipt composition.
#[path = "catalog_native_receipt_preparation.rs"]
pub mod native_receipt_preparation;
