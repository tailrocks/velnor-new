// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_model::{
    CommitSha, HelperRelease, HelperVersion, Platform, RELEASE_CONFIG_PATH, RELEASE_WORKFLOW_PATH,
    RepoPath, RepoView, Sha256,
};
use velnor_release::{ReleaseFiles, ReleasePlan};
use velnor_render::render_release;
use velnor_rust::LOCK_PATH;

fn text(view: &RepoView, path: &RepoPath) -> String {
    view.files
        .get(path)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default()
}

fn helper(view: &RepoView) -> Result<HelperRelease, Box<dyn Error>> {
    let lock = text(view, &LOCK_PATH);
    let version = lock
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .map(|value| value.trim_matches('"').to_owned())
        .ok_or("no helper version in the lock")?;
    Ok(HelperRelease::new(
        HelperVersion::new(version)?,
        CommitSha::new("a".repeat(40))?,
        Platform::ALL
            .into_iter()
            .map(|platform| Ok((platform, Sha256::new("a".repeat(64))?)))
            .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?,
    )?)
}

fn release_files(view: &RepoView) -> ReleaseFiles {
    let release_plz_toml = view
        .files
        .keys()
        .filter(|path| path.as_str().rsplit('/').next() == Some("release-plz.toml"))
        .map(|path| {
            let root = path
                .parent()
                .map_or_else(|| ".".to_owned(), |parent| parent.to_string());
            (root, text(view, path))
        })
        .collect();
    ReleaseFiles {
        release_toml: view
            .files
            .contains_key(&RELEASE_CONFIG_PATH)
            .then(|| text(view, &RELEASE_CONFIG_PATH)),
        release_plz_toml,
        mise_tomls: vec![text(
            view,
            &RepoPath::new("mise.toml").unwrap_or_else(|e| unreachable!("{e}")),
        )],
        mise_lock: text(
            view,
            &RepoPath::new("mise.lock").unwrap_or_else(|e| unreachable!("{e}")),
        ),
        ..ReleaseFiles::default()
    }
}

pub(crate) fn expected_release_nested(
    view: &RepoView,
) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    let plan = ReleasePlan::read(&release_files(view)).map_err(GenerateError::Release)?;
    let helper = helper(view).map_err(|_| GenerateError::HelperNotPinned)?;
    if !helper
        .version()
        .at_least(velnor_gen::RELEASE_HELPER_MINIMUM)
    {
        return Err(GenerateError::ReleaseHelperTooOld(helper.version().clone()));
    }
    if plan.family.is_some() && !helper.version().at_least((0, 1, 83)) {
        return Err(GenerateError::FamilyTagHelperTooOld(
            helper.version().clone(),
        ));
    }
    let mut files = render_release(&plan, &helper, "main").map_err(GenerateError::Rules)?;
    files.retain(|path, _| *path == RELEASE_WORKFLOW_PATH);
    Ok(files)
}

pub(crate) fn expected_release(
    view: &RepoView,
) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    let nested = view
        .files
        .keys()
        .any(|path| path.as_str().ends_with("/release-plz.toml"));
    if nested {
        expected_release_nested(view)
    } else {
        velnor_gen::expected_files(view)
    }
}
