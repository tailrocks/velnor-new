// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::{SECTION, private_repo};

#[test]
fn every_private_base_restore_follows_the_merge_queue_check_and_admits_a_trusted_merge_group()
-> TestResult {
    const MERGE_QUEUE_VERDICT: &str =
        "(github.event_name == 'merge_group' && steps.private-check.outputs.allowed == 'true')";
    const REPOSITORY_VERDICT: &str = "steps.private_repo.outputs.private == 'true'";
    for shape in ["", "shape = 'groups'\n"] {
        let mut repo = private_repo(
            &["Cargo.toml", "tools/Cargo.toml"],
            &format!("{shape}{SECTION}"),
            "0.1.111",
        )?;
        repo.files
            .insert(path("Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
        repo.files
            .insert(path("tools/Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
        let text = workflow(&repo)?;
        let mut restores = 0;
        for job in crate::private_sources::jobs(&text)? {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let position = |id: &str| steps.iter().position(|s| s["id"] == id);
            for (index, step) in steps.iter().enumerate() {
                let gated = step["name"].as_str().is_some_and(|name| {
                    ["Restore the sources cache", "Restore the target cache"].contains(&name)
                });
                if !gated {
                    continue;
                }
                restores += 1;
                let check = position("private-check").ok_or_else(|| format!("{shape}: {job}"))?;
                let repository =
                    position("private_repo").ok_or_else(|| format!("{shape}: {job}"))?;
                assert!(check < index && repository < index, "{shape}: {step}");
                let condition = step["if"].as_str().ok_or("restore has no condition")?;
                assert!(
                    condition.contains(MERGE_QUEUE_VERDICT),
                    "{shape}: {condition}"
                );
                assert!(
                    condition.contains(REPOSITORY_VERDICT),
                    "{shape}: {condition}"
                );
            }
        }
        assert!(restores > 0, "{shape}: no base cache restore");
    }
    Ok(())
}
