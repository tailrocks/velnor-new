// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::error::{DetectError, Reason};
use crate::roots::{patch_files, resolve};
use crate::toml_file::table;

const DEFAULT: RepoPath = RepoPath::from_static("target");
const KEY: &str = "build.target-dir";

pub(crate) fn target_dir(view: &RepoView, manifest: &RepoPath) -> Result<RepoPath, DetectError> {
    let mut configs: Vec<_> = patch_files(view, manifest)
        .into_iter()
        .filter(|(file, _, _)| *file != manifest)
        .collect();
    configs.sort_by_key(|(file, _, base)| {
        (
            base.as_ref()
                .map_or(0, |base| base.as_str().split('/').count()),
            !file.as_str().ends_with(".toml"),
        )
    });
    let mut chosen = None;
    for (file, bytes, base) in configs.into_iter().rev() {
        if let Some(text) = declared(file, bytes)? {
            chosen = Some((file, base, text));
            break;
        }
    }
    let Some((file, base, text)) = chosen else {
        return Ok(manifest.sibling(&DEFAULT));
    };
    let outside = || DetectError::invalid(file, Reason::TargetDirOutside(text.clone()));
    if text.starts_with('/') {
        return Err(outside());
    }
    let folder = resolve(base.as_ref(), &text)
        .flatten()
        .ok_or_else(outside)?;
    let folder = RepoPath::new(folder).map_err(|_| outside())?;
    let holds_root = std::iter::successors(manifest.parent(), RepoPath::parent)
        .any(|ancestor| ancestor == folder);
    if holds_root {
        Err(outside())
    } else {
        Ok(folder)
    }
}

fn declared(file: &RepoPath, bytes: &[u8]) -> Result<Option<String>, DetectError> {
    let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
    let Some(value) = parsed
        .get("build")
        .and_then(|build| build.get("target-dir"))
    else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|text| Some(text.to_owned()))
        .ok_or_else(|| {
            DetectError::invalid(
                file,
                Reason::InvalidValue {
                    key: KEY,
                    value: value.to_string(),
                },
            )
        })
}
