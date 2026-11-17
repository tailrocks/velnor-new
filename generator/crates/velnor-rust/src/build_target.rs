// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::error::{DetectError, Reason};
use crate::target_dir::nearest_first;
use crate::toml_file::table;

const HOST: &str = "host-tuple";
const KEY: &str = "build.target";

pub(crate) fn build_targets(
    view: &RepoView,
    manifest: &RepoPath,
) -> Result<Vec<(RepoPath, String)>, DetectError> {
    config_targets(view, manifest).map(|(named, _)| named)
}

fn config_targets(
    view: &RepoView,
    manifest: &RepoPath,
) -> Result<(Vec<(RepoPath, String)>, bool), DetectError> {
    let mut named = Vec::new();
    let mut kind = None;
    let mut scalar_seen = false;
    let mut configured = false;
    for (file, bytes, _) in nearest_first(view, manifest) {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        let Some(value) = parsed.get("build").and_then(|build| build.get("target")) else {
            continue;
        };
        configured = true;
        let invalid = || {
            DetectError::invalid(
                file,
                Reason::InvalidValue {
                    key: KEY,
                    value: value.to_string(),
                },
            )
        };
        match value {
            toml::Value::String(one) => {
                if kind == Some(true) {
                    return Err(invalid());
                }
                kind = Some(false);
                if !scalar_seen {
                    named.push((file.clone(), one.clone()));
                    scalar_seen = true;
                }
            }
            toml::Value::Array(many) => {
                if kind == Some(false) {
                    return Err(invalid());
                }
                kind = Some(true);
                for item in many {
                    let name = item.as_str().ok_or_else(invalid)?;
                    if !scalar_seen {
                        named.push((file.clone(), name.to_owned()));
                    }
                }
            }
            _ => return Err(invalid()),
        }
    }
    named.retain(|(_, name)| name != HOST);
    Ok((named, configured))
}

pub(crate) fn sets_build_target(view: &RepoView, manifest: &RepoPath) -> Result<bool, DetectError> {
    config_targets(view, manifest).map(|(_, configured)| configured)
}
