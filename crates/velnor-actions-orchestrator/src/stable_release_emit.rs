use crate::{OrchestratorError, prepare::GenerationPreparation, utf8::strings_of};
use std::ffi::OsStr;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
pub(crate) fn release_plz_argv(
    _prep: &GenerationPreparation,
) -> Result<Vec<String>, OrchestratorError> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::ReleasePlz],
        OsStr::new("release-plz"),
        Vec::new(),
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })?;
    strings_of(request.argv(&ToolCatalog::pinned()))
        .map_err(|problem| OrchestratorError::Contract { problem })
}
