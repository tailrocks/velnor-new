// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

const MAX_GH_STDOUT_BYTES: usize = 1 << 20;

pub(crate) struct GhDownload;

impl GhDownload {
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
