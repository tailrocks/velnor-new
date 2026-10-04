//! Project native support sources into the renderer's neutral output record.

use velnor_actions_contract::config::NativeDesktopProfile;
use velnor_actions_native::swift;
use velnor_actions_workflow_renderer::{RenderError, RenderedFile};

/// Assemble source-owned helpers and one validated native profile.
/// # Errors
/// Rejects invalid profiles, unmanaged destinations, or marker versions.
pub fn desktop_helper_files(
    profile: &NativeDesktopProfile,
    profile_path: &str,
    version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    let bundle = swift::support_sources(version).map_err(RenderError::Contract)?;
    let mut files = bundle
        .files()
        .iter()
        .map(|file| RenderedFile {
            path: file.path().to_owned(),
            bytes: file.source().to_owned(),
        })
        .collect::<Vec<_>>();
    let profile =
        swift::profile_file(profile_path, profile, version).map_err(RenderError::Contract)?;
    files.push(RenderedFile {
        path: profile.path().to_owned(),
        bytes: profile.source().to_owned(),
    });
    Ok(files)
}
