use std::ffi::OsString;
use std::path::PathBuf;

use super::resolve_action_owned_mbx_in;

#[path = "../tests/support/action_mbx_fixture.rs"]
mod fixture;

#[test]
fn action_owned_mbx_resolution_uses_first_absolute_executable_path() -> Result<(), String> {
    let root = crate::test_temp_dir::unique_temp_dir("action-mbx-resolution")
        .map_err(|error| error.to_string())?;
    let _cleanup = fixture::TemporaryTree::new(root.clone());
    let non_executable =
        fixture::new_folder(&root, "not-executable").map_err(|error| error.to_string())?;
    let action_bin = fixture::new_folder(&root, "action-bin").map_err(|error| error.to_string())?;
    let rejected = non_executable.join("mbx");
    let expected = action_bin.join("mbx");
    fixture::save_bytes(&rejected, b"not executable").map_err(|error| error.to_string())?;
    fixture::save_bytes(&expected, b"action executable").map_err(|error| error.to_string())?;
    fixture::make_executable(&expected).map_err(|error| error.to_string())?;
    let path = std::env::join_paths([PathBuf::from("relative-entry"), non_executable, action_bin])
        .map_err(|error| error.to_string())?;

    let resolved = resolve_action_owned_mbx_in(&[(OsString::from("PATH"), path)])
        .map_err(|error| error.to_string())?;
    assert_eq!(resolved.as_os_str(), expected.as_os_str());
    Ok(())
}

#[test]
fn action_owned_mbx_resolution_rejects_missing_path_entries() {
    let error = resolve_action_owned_mbx_in(&[]).expect_err("missing PATH must fail closed");
    assert!(
        error
            .to_string()
            .contains("action_owned_binary_not_found_on_path")
    );
}
