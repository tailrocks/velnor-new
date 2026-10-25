//! Do a check of each pinned source and its recorded identity.

use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn every_pinned_catalog_source_passes_identity_validation() {
    let catalog = ToolCatalog::pinned();

    for tool in PinnedTool::ALL {
        let mut identity = catalog.tool_identity(tool);
        // This digest is test data for the source check.
        identity.digest = "a".repeat(64);
        identity.validate("catalog").unwrap_or_else(|error| {
            panic!(
                "The source for {} is not correct ({}): {error}",
                tool.tool_name(),
                identity.source
            )
        });
    }
}
