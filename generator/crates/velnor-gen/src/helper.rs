// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{FullRun, HelperRelease, HelperVersion, OnMain, RepoFacts};

use crate::{COMPATIBLE_HELPER_SERIES, GenerateError};

pub(crate) fn check_helper(facts: &RepoFacts) -> Result<&HelperRelease, GenerateError> {
    let release = facts
        .helper
        .as_ref()
        .ok_or(GenerateError::HelperNotPinned)?;
    require_compatible(release.version())?;
    if (facts.config.on_main == OnMain::Everything || facts.config.full_run != FullRun::Off)
        && !release.version().at_least((0, 1, 81))
    {
        return Err(GenerateError::InputsBuildHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.crates_mirror.is_some() && !release.version().at_least((0, 1, 81)) {
        return Err(GenerateError::MirrorHelperTooOld(release.version().clone()));
    }
    Ok(release)
}

pub fn require_compatible(version: &HelperVersion) -> Result<(), GenerateError> {
    if version.series() == Some(COMPATIBLE_HELPER_SERIES) {
        Ok(())
    } else {
        Err(GenerateError::IncompatibleHelper(version.clone()))
    }
}
