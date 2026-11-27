// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::mise_world::{World, git};

impl World {
    pub(crate) fn trusted_facts(&self) -> Result<Option<PathBuf>> {
        let workspace = self.workspace();
        if !workspace.join(".git").exists() {
            return Ok(None);
        }
        let sha = git(&workspace, &["rev-parse", "HEAD"])?;
        let mirror = self.mirror();
        if !mirror.join("config").is_file() {
            std::fs::create_dir_all(&mirror).context("create the mirror")?;
            git(&mirror, &["init", "--quiet", "--bare"])?;
        }
        let url = mirror.to_string_lossy();
        let push = ["-c", "core.fsmonitor=false", "push", "--quiet", "--force"];
        git(
            &workspace,
            &[&push[..], &[&url, "HEAD:refs/heads/main"]].concat(),
        )?;
        let facts = self.root.join("trusted.json");
        std::fs::write(
            &facts,
            format!(
                r#"{{"repository_id": 1, "repository": "tailrocks/velnor-new",
"server_url": "file:///velnor-test", "default_branch": "main", "sha": "{sha}",
"run_id": 1, "run_attempt": 1, "event": {{"kind": "branch", "branch": "main"}}}}"#
            ),
        )
        .context("write the trusted facts")?;
        Ok(Some(facts))
    }
}
