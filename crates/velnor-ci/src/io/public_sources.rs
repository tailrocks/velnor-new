// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_rust::{LockAdmission, PinnedGit, merge_pinned};

use crate::cli_cache::PublicSourcesArgs;
use crate::error::CiError;
use crate::public_probe::{Deadline, Verdicts};

pub(super) fn run(args: &PublicSourcesArgs) -> Result<String, CiError> {
    let root = super::current_dir()?;
    let manifests = super::cache_paths::list("VELNOR_CACHE_MANIFESTS")?
        .unwrap_or_else(|| args.manifest_path.clone());
    let token = super::public_repo::env_token();
    let deadline = Deadline::after(std::time::Duration::from_secs(args.public_probe_seconds));
    let mut verdicts = Verdicts::default();
    let mut log: Vec<String> = Vec::new();
    let (declared, _) = super::cache_sources::declared_only(&root, repositories(&root, &manifests))
        .unwrap_or_default();
    for pinned in declared {
        let repository = pinned.repository.clone();
        let name = format!("github.com/{}/{}", repository.owner, repository.name);
        let public = match super::public_repo::verify(
            &args.public_probe_api,
            &pinned,
            token.as_deref(),
            deadline,
        ) {
            Ok(credentials) => {
                log.push(format!(
                    "git dependency {name} is public ({})\n",
                    super::cache_sources::describe(credentials)
                ));
                true
            }
            Err(reason) => {
                log.push(format!(
                    "::notice::git dependency {name} is not proven public ({reason})\n"
                ));
                false
            }
        };
        verdicts.record(&repository, public);
    }
    super::append_outputs(&format!("verdicts={}\n", verdicts.render()))?;
    Ok(log.concat())
}

fn repositories(root: &Path, manifests: &[PathBuf]) -> Vec<PinnedGit> {
    let mut found: Vec<PinnedGit> = Vec::new();
    for manifest in manifests {
        let Some(directory) = root.join(manifest).parent().map(Path::to_path_buf) else {
            continue;
        };
        let Some(LockAdmission::ProbeGithub(listed)) =
            super::cache_sources::read_lock(&directory.join("Cargo.lock"))
        else {
            continue;
        };
        merge_pinned(&mut found, listed);
    }
    found
}
