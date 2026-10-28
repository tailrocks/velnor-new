// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};

use serde::Deserialize;
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};

use crate::{OrchestratorError, github_authority_read, internal::internal};

const RESPONSE_LIMIT: usize = 16 * 1024 * 1024;

pub(super) fn get_json<T: for<'de> Deserialize<'de>>(
    route: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<T, OrchestratorError> {
    let mut args = vec![OsString::from("api")];
    args.extend(github_authority_read::arguments(route)?);
    let command = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("image_publication_gh_unavailable"))?
        .command_with_github_token(&ToolCatalog::pinned(), token)
        .map_err(|_| internal("image_publication_gh_token"))?;
    let (output, _) = command
        .run_quiescent_until(RESPONSE_LIMIT, deadline)
        .map_err(|_| internal("image_publication_github_api"))?;
    if !output.success {
        return Err(internal("image_publication_github_api"));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| internal("image_publication_github_json"))
}
