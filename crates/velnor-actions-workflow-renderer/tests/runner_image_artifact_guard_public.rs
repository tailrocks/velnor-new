// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;

use super::super::gc1_t42::source_preview_artifact_guard_public as guard;
use super::gc1_t48_runner_family::family_files;

#[test]
fn runner_image_workflows_guard_every_download_by_exact_artifact_id() -> Result<(), Box<dyn Error>>
{
    let texts = family_files()?
        .into_iter()
        .map(|file| (file.path, file.bytes))
        .collect();
    let guards = guard::guarded_downloads_in(texts)?;
    assert!(guards.len() >= 4, "found {} guards", guards.len());
    guard::assert_guards_reject_bad_ids(guards)
}
