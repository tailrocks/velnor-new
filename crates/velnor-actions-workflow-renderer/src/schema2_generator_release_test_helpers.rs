use super::Failure;
use std::error::Error;
use std::path::Path;

pub(super) fn expects_draft_creation(case: Failure) -> bool {
    matches!(
        case,
        Failure::None
            | Failure::WrongDraftReleaseId
            | Failure::WrongDraftRepository
            | Failure::WrongDraftApiPath
            | Failure::WrongDraftTag
            | Failure::WrongDraftSource
            | Failure::WrongDraftDigest
            | Failure::WrongDraftSize
            | Failure::WrongDraftInventory
            | Failure::UnknownDraftAsset
            | Failure::WrongDraftAssetName
            | Failure::WrongDraftAssetState
            | Failure::UntaggedDraftUrls
            | Failure::ChangedReleaseId
            | Failure::WrongPublishedDigest
            | Failure::WrongPublishedAssetUrl
            | Failure::WrongPublishedUrl
            | Failure::MutablePublished
            | Failure::TagMovedBeforePublish
    )
}

pub(super) fn is_draft_validation_failure(case: Failure) -> bool {
    matches!(
        case,
        Failure::WrongDraftReleaseId
            | Failure::WrongDraftRepository
            | Failure::WrongDraftApiPath
            | Failure::WrongDraftTag
            | Failure::WrongDraftSource
            | Failure::WrongDraftDigest
            | Failure::WrongDraftSize
            | Failure::WrongDraftInventory
            | Failure::UnknownDraftAsset
            | Failure::WrongDraftAssetName
            | Failure::WrongDraftAssetState
    )
}

pub(super) fn install_mock_gh(root: &Path) -> Result<(), Box<dyn Error>> {
    super::fake_commands::install_mock_gh(root)
}

pub(super) fn path_with_mock_gh(root: &Path) -> Result<std::ffi::OsString, Box<dyn Error>> {
    Ok(super::with_required_tool_path(&[root.join("mock-bin")])?)
}

pub(super) fn preflight_mode(case: Failure) -> &'static str {
    match case {
        Failure::UnauthorizedTag => "401",
        Failure::ForbiddenTag => "403",
        Failure::TransientTag => "transient",
        _ => "404",
    }
}

pub(super) fn assert_no_release_created(root: &Path) {
    assert!(!root.join("created-tag").exists());
}
