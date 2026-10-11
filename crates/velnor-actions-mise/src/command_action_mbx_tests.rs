use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::resolve_action_owned_mbx_in;

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("action_mbx_path_cleanup_failed:{:?}", error.kind());
        }
    }
}

#[test]
fn action_owned_mbx_resolution_uses_first_absolute_executable_path() -> Result<(), String> {
    let root = crate::test_temp_dir::unique_temp_dir("action-mbx-resolution")
        .map_err(|error| error.to_string())?;
    let _cleanup = Cleanup(root.clone());
    let non_executable = root.join("not-executable");
    let action_bin = root.join("action-bin");
    std::fs::create_dir(&non_executable).map_err(|error| error.to_string())?;
    std::fs::create_dir(&action_bin).map_err(|error| error.to_string())?;
    let rejected = non_executable.join("mbx");
    let expected = action_bin.join("mbx");
    std::fs::write(&rejected, b"not executable").map_err(|error| error.to_string())?;
    std::fs::write(&expected, b"action executable").map_err(|error| error.to_string())?;
    std::fs::set_permissions(&expected, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
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
