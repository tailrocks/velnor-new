#![cfg(unix)]
//! Pure diagnostic encoding proof; no filesystem or subprocess claim.
use super::probe_failure;
use crate::MiseError;
use crate::command::ProcessOutput;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[test]
fn invalid_os_bytes_are_escaped_without_filesystem_access() -> Result<(), MiseError> {
    let program = Path::new(std::ffi::OsStr::from_bytes(b"/owned/docker-\n\x1b\"\\\xff"));
    let result = ProcessOutput {
        stdout: b"PRIVATE_STDOUT".to_vec(),
        stderr: Vec::new(),
        code: Some(23),
        signal: None,
        success: false,
    };
    let error = probe_failure(program, &["--version"], &result);
    let MiseError::InvalidStepInput { field, value } = error else {
        return Err(error);
    };
    assert_eq!(field, "container_probe");
    assert!(value.contains(r#"program="/owned/docker-\n\x1b\"\\\xff""#));
    assert!(value.contains("code=Some(23), signal=None"));
    assert!(!value.contains("PRIVATE_STDOUT"));
    assert!(!value.contains('\n'));
    assert!(!value.contains('\x1b'));
    Ok(())
}
