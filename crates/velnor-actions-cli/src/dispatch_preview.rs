//! Preview-directory display path for `generate --output-dir`.

use std::path::{Path, PathBuf};

/// Absolute preview path for display (unresolved when missing).
pub(crate) fn absolute_preview(cwd: &Path, dir: &Path) -> PathBuf {
    let joined = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    };
    joined.canonicalize().unwrap_or(joined)
}
