//! Keep nested Cargo checks outside the source worktree.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(super) fn cargo_target_dir() -> std::io::Result<PathBuf> {
    Ok(selected_target_dir(
        std::env::var_os("CARGO_TARGET_DIR"),
        std::env::var_os("CARGO_TARGET_TMPDIR"),
        &std::env::current_dir()?,
        &std::env::temp_dir(),
        std::process::id(),
    ))
}

fn selected_target_dir(
    configured: Option<OsString>,
    test_temp: Option<OsString>,
    current_dir: &Path,
    temp_dir: &Path,
    process_id: u32,
) -> PathBuf {
    let target = configured.or(test_temp).map_or_else(
        || temp_dir.join(format!("velnor-test-registration-{process_id}")),
        PathBuf::from,
    );
    if target.is_absolute() {
        target
    } else {
        current_dir.join(target)
    }
}

#[test]
fn nested_cargo_target_is_absolute_and_anchored_before_chdir() {
    let checkout = Path::new("/checkout");
    let temp = Path::new("/task-tmp");
    assert_eq!(
        selected_target_dir(
            Some(OsString::from("cache/cargo")),
            None,
            checkout,
            temp,
            17,
        ),
        checkout.join("cache/cargo")
    );
    assert_eq!(
        selected_target_dir(
            None,
            Some(OsString::from("/outer/target/tmp")),
            checkout,
            temp,
            17
        ),
        Path::new("/outer/target/tmp")
    );
    assert_eq!(
        selected_target_dir(None, None, checkout, temp, 17),
        temp.join("velnor-test-registration-17")
    );
}
