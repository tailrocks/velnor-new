use super::*;
use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

fn temporary_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-cosign-{}-{label}",
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn cosign_admission_rejects_wrong_digest() {
    assert!(!valid_cosign_bytes(b"not the pinned Cosign binary"));
}

#[test]
fn staged_cosign_path_rejects_directories_and_symlinks() -> Result<(), OrchestratorError> {
    let directory = temporary_path("directory");
    fs::create_dir(&directory).map_err(|_| internal("test_directory_create_failed"))?;
    assert!(read_regular_file(&directory, 1024).is_err());
    fs::remove_dir(&directory).map_err(|_| internal("test_directory_remove_failed"))?;

    let target = temporary_path("target");
    let link = temporary_path("link");
    fs::write(&target, b"wrong executable bytes")
        .map_err(|_| internal("test_file_write_failed"))?;
    symlink(&target, &link).map_err(|_| internal("test_symlink_create_failed"))?;
    assert!(read_regular_file(&link, 1024).is_err());
    fs::remove_file(&link).map_err(|_| internal("test_link_remove_failed"))?;
    fs::remove_file(&target).map_err(|_| internal("test_target_remove_failed"))?;
    Ok(())
}
