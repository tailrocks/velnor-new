use super::mbx_identity_digest;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn mbx_identity_tracks_the_project_version() {
    let pinned = ToolCatalog::pinned();
    let project = pinned
        .clone()
        .with_project_tool(PinnedTool::MrBoxington, "0.3.1")
        .expect("exact project version");

    assert_ne!(mbx_identity_digest(&pinned), mbx_identity_digest(&project));
    assert_ne!(
        mbx_identity_digest(&project),
        mbx_identity_digest(
            &ToolCatalog::pinned()
                .with_project_tool(PinnedTool::MrBoxington, "0.3.2")
                .expect("exact project version")
        ),
    );
}
