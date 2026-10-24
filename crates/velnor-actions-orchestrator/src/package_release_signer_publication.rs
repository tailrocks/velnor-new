use std::path::Path;

use crate::OrchestratorError;

pub(super) fn hold_signed_directory_for_publication<T>(
    directory: &Path,
    custody_deadline: velnor_actions_mise::CheckDeadline,
    verify: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<(T, crate::filesystem_owner::held_directory::HeldDirectory), OrchestratorError> {
    crate::package_release_publish::budget::with_default_scope(|_| {
        crate::package_release_publish::budget::hold_signed_directory(
            directory,
            custody_deadline,
            verify,
        )
    })
}
