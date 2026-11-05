// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::OsStr;
use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::maintenance_features::owner_feature_repo;

const SELF_REPOSITORY_IGNORE: &str = "# zizmor: ignore[self-repository]";

fn local_uses_lines(bytes: &str) -> Vec<&str> {
    bytes
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix("- ").unwrap_or(line);
            line.strip_prefix("uses:")
                .is_some_and(|value| value.trim_start().trim_start_matches('"').starts_with("./"))
        })
        .collect()
}

#[test]
fn every_local_uses_in_generated_workflows_carries_the_self_repository_annotation()
-> Result<(), Box<dyn Error>> {
    let mut seen = 0;
    let mut unannotated = Vec::new();
    for policy in [
        WorkflowPolicy::ConsumerV1,
        WorkflowPolicy::VelnorRepositoryV1,
    ] {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        for file in &tree.files {
            if Path::new(&file.path).extension() != Some(OsStr::new("yml")) {
                continue;
            }
            for line in local_uses_lines(&file.bytes) {
                seen += 1;
                if !line.contains(SELF_REPOSITORY_IGNORE) {
                    unannotated.push(format!("{policy:?} {}: {}", file.path, line.trim()));
                }
            }
        }
    }
    assert!(seen > 0, "no local uses were generated");
    assert!(
        unannotated.is_empty(),
        "local uses without the self-repository annotation:\n{}",
        unannotated.join("\n")
    );
    Ok(())
}
