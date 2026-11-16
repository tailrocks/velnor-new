// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BuildTarget, RepoPath, RepoView};

use crate::error::{DetectError, Reason};
use crate::roots::{patch_files, resolve};
use crate::toml_file::table;

const DEFAULT: RepoPath = RepoPath::from_static("target");
const KEY: &str = "build.target-dir";

type Found<'v> = (&'v RepoPath, Option<RepoPath>, toml::Value);

pub(crate) fn target_dir(view: &RepoView, manifest: &RepoPath) -> Result<RepoPath, DetectError> {
    let Some((file, base, value)) = nearest(view, manifest, "target-dir")? else {
        return Ok(manifest.sibling(&DEFAULT));
    };
    let text = value.as_str().map(str::to_owned).ok_or_else(|| {
        DetectError::invalid(
            file,
            Reason::InvalidValue {
                key: KEY,
                value: value.to_string(),
            },
        )
    })?;
    let outside = || DetectError::invalid(file, Reason::TargetDirOutside(text.clone()));
    if text.starts_with('/') {
        return Err(outside());
    }
    let segments = resolve(base.as_ref(), &text).ok_or_else(outside)?;
    let folder = segments.join("/");
    let folder = RepoPath::new(folder).map_err(|_| outside())?;
    let holds_root = std::iter::successors(manifest.parent(), RepoPath::parent)
        .any(|ancestor| ancestor == folder);
    if holds_root {
        Err(outside())
    } else {
        Ok(folder)
    }
}

pub(crate) fn build_target(
    view: &RepoView,
    manifest: &RepoPath,
) -> Result<BuildTarget, DetectError> {
    let Some((_, _, value)) = nearest(view, manifest, "target")? else {
        return Ok(BuildTarget::Host);
    };
    let one = match &value {
        toml::Value::String(triple) => Some(triple.as_str()),
        toml::Value::Array(items) if items.len() == 1 => items[0].as_str(),
        _ => None,
    };
    Ok(one
        .filter(|triple| {
            !triple.is_empty()
                && !triple.to_ascii_lowercase().ends_with(".json")
                && !triple.contains(['/', '\\'])
                && !triple.starts_with('.')
        })
        .map_or(BuildTarget::Unsupported, |triple| {
            BuildTarget::Triple(triple.to_owned())
        }))
}

fn nearest<'v>(
    view: &'v RepoView,
    manifest: &RepoPath,
    key: &str,
) -> Result<Option<Found<'v>>, DetectError> {
    let mut configs: Vec<_> = patch_files(view, manifest)
        .into_iter()
        .filter(|(file, _, _)| *file != manifest)
        .collect();
    configs.sort_by_key(|(file, _, base)| {
        (
            base.as_ref()
                .map_or(0, |base| base.as_str().split('/').count()),
            file.as_str().strip_suffix(".toml").is_none(),
        )
    });
    for (file, bytes, base) in configs.into_iter().rev() {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        if let Some(value) = parsed.get("build").and_then(|build| build.get(key)) {
            return Ok(Some((file, base, value.clone())));
        }
    }
    Ok(None)
}
