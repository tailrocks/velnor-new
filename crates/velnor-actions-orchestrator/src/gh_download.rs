//! Pinned `gh` runner for exact Actions-artifact downloads.
//!
//! Callers build fixed argv (exact names, explicit `--repo`); this module
//! only runs it under the pinned catalog and bounds the captured stdout.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

/// Maximum accepted `gh` stdout bytes for one download call.
///
/// Download receipts are kilobytes; one megabyte fails closed on runaway
/// output instead of buffering megabytes no caller needs. (The spawn
/// layer caps pipes at 8 MiB; this is the tighter call-site bound.)
const MAX_GH_STDOUT_BYTES: usize = 1 << 20;

/// Pinned `gh` invocation over caller-fixed argv.
pub(crate) struct GhDownload;

impl GhDownload {
    /// Run fixed `gh` args under the pinned catalog in `root`.
    ///
    /// # Errors
    ///
    /// Returns `gh_download_failed` for any spawn, exit, size, or
    /// decoding failure.
    pub(crate) fn run(
        catalog: &ToolCatalog,
        root: &Path,
        args: Vec<OsString>,
    ) -> Result<String, String> {
        let failed = || "gh_download_failed".to_owned();
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
            .map_err(|err| err.to_string())?;
        let output = exec
            .command(catalog)
            .map_err(|err| err.to_string())?
            .with_cwd(PathBuf::from(root))
            .run()
            .map_err(|_| failed())?;
        if !output.success || output.stdout.len() > MAX_GH_STDOUT_BYTES {
            return Err(failed());
        }
        output.stdout_text("gh").map_err(|_| failed())
    }
}
