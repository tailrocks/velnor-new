//! Observed programs link into the check home `bin` directory.

use std::ffi::OsStr;

use velnor_actions_orchestrator_check_acquisition::acquisition::link_program;

#[test]
fn link_program_creates_live_symlink() {
    let temp = tempfile::TempDir::new().expect("temp");
    let target = temp.path().join("program");
    std::fs::write(&target, b"bytes").expect("target");
    let destination = temp.path().join("linked");
    link_program(target.as_os_str(), &destination).expect("link");
    assert_eq!(std::fs::read(&destination).expect("read"), b"bytes");
    assert!(
        std::fs::symlink_metadata(&destination)
            .expect("meta")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn link_program_refuses_existing_destination() {
    let temp = tempfile::TempDir::new().expect("temp");
    let target = temp.path().join("program");
    std::fs::write(&target, b"bytes").expect("target");
    let destination = temp.path().join("linked");
    std::fs::write(&destination, b"taken").expect("taken");
    assert!(link_program(target.as_os_str(), &destination).is_err());
    assert_eq!(std::fs::read(&destination).expect("read"), b"taken");
}

#[test]
fn link_program_accepts_os_str_programs() {
    let temp = tempfile::TempDir::new().expect("temp");
    let program = OsStr::new("/bin/sh");
    let destination = temp.path().join("sh");
    if link_program(program, &destination).is_ok() {
        assert!(
            std::fs::symlink_metadata(&destination)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
    }
}
