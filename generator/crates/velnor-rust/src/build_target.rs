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
    let mut named = Vec::new();
    for (file, bytes, _) in nearest_first(view, manifest) {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        let Some(value) = parsed.get("build").and_then(|build| build.get("target")) else {
            continue;
        };
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
                named.push((file.clone(), one.clone()));
                break;
            }
            toml::Value::Array(many) => {
                for item in many {
                    let name = item.as_str().ok_or_else(invalid)?;
                    named.push((file.clone(), name.to_owned()));
                }
            }
            _ => return Err(invalid()),
        }
    }
    named.retain(|(_, name)| name != HOST);
    Ok(named)
}

pub(crate) fn sets_build_target(view: &RepoView, manifest: &RepoPath) -> Result<bool, DetectError> {
    build_targets(view, manifest).map(|named| !named.is_empty())
}
