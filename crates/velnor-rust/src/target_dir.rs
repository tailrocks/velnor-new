// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, RootSlug, RustRoot};

use crate::error::{DetectError, Reason};
use crate::metadata_file::pin_at_least;
use crate::roots::{patch_files, resolve};
use crate::toml_file::table;

const DEFAULT: RepoPath = RepoPath::from_static("target");
const KEY: &str = "build.target-dir";
/// The folder below a repository's `target` that holds the build folders of the roots outside the
/// repository root. The helper's unit scan leaves it out of the root above.
pub const NESTED_ROOTS_FOLDER: &str = "velnor-roots";
/// The first `velnor-ci` that works out the same default folder for a root outside the repository
/// root, so a lock that pins an older one keeps the folder beside the manifest.
// helper-gate: first in tools-v0.1.107 via 8c68e540ed
const NESTED_TARGET_HELPER: (u64, u64, u64) = (0, 1, 107);

type Found<'v> = (&'v RepoPath, Option<RepoPath>, toml::Value);

pub(crate) fn target_dir(view: &RepoView, manifest: &RepoPath) -> Result<RepoPath, DetectError> {
    let (folder, _) = target_dir_source(view, manifest)?;
    let overlaps = |path: &RepoPath| {
        path == &folder
            || path.as_str().starts_with(&format!("{folder}/"))
            || folder.as_str().starts_with(&format!("{path}/"))
    };
    if folder
        .as_str()
        .split('/')
        .any(|part| matches!(part, ".git" | ".cargo"))
        || view.tracked_paths.iter().any(overlaps)
        || view.rust_metadata.keys().any(overlaps)
    {
        return Err(DetectError::invalid(
            manifest,
            Reason::TargetDirOverlap(folder.to_string()),
        ));
    }
    Ok(folder)
}

pub(crate) fn target_dir_source<'v>(
    view: &'v RepoView,
    manifest: &RepoPath,
) -> Result<(RepoPath, Option<&'v RepoPath>), DetectError> {
    if let Some((file, ..)) = nearest(view, manifest, "build-dir")? {
        return Err(DetectError::invalid(
            file,
            Reason::BuildDirKey("build.build-dir"),
        ));
    }
    let Some((file, base, value)) = nearest(view, manifest, "target-dir")? else {
        return Ok((default_dir(view, manifest), None));
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
    if text.starts_with(['/', '~']) || text.contains(['{', '}']) {
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
        Ok((folder, Some(file)))
    }
}

/// The folder a root builds into when no `build.target-dir` names one: `target` beside the
/// manifest for the repository root, and `target/velnor-roots/<root slug>` for any other root.
/// Cargo walks every package directory for its fingerprint, and a lane that writes `target/`
/// inside a nested package races that walk. This is the one place the default is decided: the
/// cache paths, the `CARGO_TARGET_DIR` of each step, the saves and the helper's trim read it.
fn default_dir(view: &RepoView, manifest: &RepoPath) -> RepoPath {
    if manifest.parent().is_some() && pin_at_least(view, NESTED_TARGET_HELPER) {
        return nested_default(manifest);
    }
    manifest.sibling(&DEFAULT)
}

#[must_use]
pub fn default_folder(manifest: &RepoPath) -> RepoPath {
    if manifest.parent().is_some() {
        nested_default(manifest)
    } else {
        manifest.sibling(&DEFAULT)
    }
}

fn nested_default(manifest: &RepoPath) -> RepoPath {
    RepoPath::new(format!(
        "target/{NESTED_ROOTS_FOLDER}/{}",
        RootSlug::of(manifest)
    ))
    .unwrap_or_else(|_| manifest.sibling(&DEFAULT))
}

/// Whether the root builds into a default folder (either rule) and not one a Cargo
/// configuration chose.
pub(crate) fn is_default(root: &RustRoot) -> bool {
    root.target_dir == root.manifest.sibling(&DEFAULT)
        || root.target_dir == nested_default(&root.manifest)
}

fn nearest<'v>(
    view: &'v RepoView,
    manifest: &RepoPath,
    key: &str,
) -> Result<Option<Found<'v>>, DetectError> {
    for (file, bytes, base) in nearest_first(view, manifest) {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        if let Some(value) = parsed.get("build").and_then(|build| build.get(key)) {
            return Ok(Some((file, base, value.clone())));
        }
    }
    Ok(None)
}

pub(crate) fn nearest_first<'v>(
    view: &'v RepoView,
    manifest: &RepoPath,
) -> Vec<(&'v RepoPath, &'v [u8], Option<RepoPath>)> {
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
    configs.reverse();
    configs
}
