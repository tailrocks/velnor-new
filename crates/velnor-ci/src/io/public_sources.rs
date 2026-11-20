// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_model::GitRepository;
use velnor_rust::LockAdmission;

use crate::cli_cache::PublicSourcesArgs;
use crate::error::CiError;
use crate::public_probe::Verdicts;

/// `velnor-ci cache public-sources`: looks each commit-pinned github.com git dependency of the
/// checkout up once and writes the verdicts of this run as a step output. A lane's `cache decide`
/// reads them instead of asking the API again. The step only records what the API said; `decide`
/// still checks every lock and the environment itself, and probes by itself for a repository the
/// verdicts do not show as public, so a failed or missing lookup here never fails the step.
pub(super) fn run(args: &PublicSourcesArgs) -> Result<String, CiError> {
    let root = super::current_dir()?;
    let manifests = super::cache_paths::list("VELNOR_CACHE_MANIFESTS")?
        .unwrap_or_else(|| args.manifest_path.clone());
    let token = super::public_repo::env_token();
    let mut verdicts = Verdicts::default();
    let mut log: Vec<String> = Vec::new();
    for repository in repositories(&root, &manifests) {
        let name = format!("github.com/{}/{}", repository.owner, repository.name);
        let public = match super::public_repo::check(
            &args.public_probe_api,
            &repository,
            token.as_deref(),
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

fn repositories(root: &Path, manifests: &[PathBuf]) -> Vec<GitRepository> {
    let mut found: Vec<GitRepository> = Vec::new();
    for manifest in manifests {
        let Some(directory) = root.join(manifest).parent().map(Path::to_path_buf) else {
            continue;
        };
        let Some(LockAdmission::ProbeGithub(listed)) =
            super::cache_sources::read_lock(&directory.join("Cargo.lock"))
        else {
            continue;
        };
        for repository in listed {
            if !found.contains(&repository) {
                found.push(repository);
            }
        }
    }
    found
}
