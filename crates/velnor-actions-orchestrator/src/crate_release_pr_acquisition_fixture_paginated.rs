// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{AcquisitionRequest, Download, Routes, acquire_preparation};
use crate::crate_release_github::NativeGitHubAdmissionRead;
use crate::crate_release_pr_artifact::PreparationArtifact;
use serde_json::{Value, json};

pub(super) fn check(
    request: &AcquisitionRequest<'_>,
    github: &NativeGitHubAdmissionRead,
    download: &Download,
    routes: &Routes,
    prefix: &str,
    expected: &PreparationArtifact,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = format!("{prefix}/actions/runs/42/artifacts?per_page=100");
    let original = routes
        .lock()
        .map_err(|_| "routes")?
        .get(&base)
        .cloned()
        .ok_or("list")?;
    let selected = original["artifacts"][0].clone();
    let mut entries: Vec<Value> = (0..200)
        .map(|i| json!({"id":1000+i,"name":format!("other-{i}")}))
        .collect();
    entries.push(selected);
    {
        let mut values = routes.lock().map_err(|_| "routes")?;
        for (page, items) in entries.chunks(100).enumerate() {
            let route = if page == 0 {
                base.clone()
            } else {
                format!("{base}&page={}", page + 1)
            };
            values.insert(
                route,
                json!({"total_count":entries.len(),"artifacts":items}),
            );
        }
    }
    let verified = acquire_preparation(request, github, download)?;
    assert_eq!(verified.artifact, *expected);
    let mut values = routes.lock().map_err(|_| "routes")?;
    values.insert(base.clone(), original);
    values.remove(&format!("{base}&page=2"));
    values.remove(&format!("{base}&page=3"));
    Ok(())
}
