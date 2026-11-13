// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RepoView};
use velnor_render::render;
use velnor_rust::{DetectError, detect, plan};

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, DetectError> {
    Ok(render(&plan(&detect(view)?)))
}
