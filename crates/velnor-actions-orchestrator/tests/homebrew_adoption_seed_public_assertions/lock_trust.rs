// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path};

use velnor_actions_orchestrator::{OrchestratorError, finalized_jobs};

pub(crate) fn assert_lock_trust_rejections(
    root: &Path,
    lock_path: &Path,
    original: &str,
) -> Result<(), Box<dyn Error>> {
    let (mise_url, version_line, stale_problem) = current_mise_mutation_values();
    let mutations = [
        (
            "alternate published version",
            "0.1.72",
            "0.1.71",
            "lock_mismatch:version:0.1.71:0.1.72",
        ),
        (
            "alternate source commit",
            "ba363080a7707684fe8f2094851fa513687de73b",
            "ca363080a7707684fe8f2094851fa513687de73b",
            "lock_mismatch:commit:ca363080a7707684fe8f2094851fa513687de73b:ba363080a7707684fe8f2094851fa513687de73b",
        ),
        (
            "Linux generator digest",
            "ae9b45357917fb3dd279b2fb1d0f6df527ca1a3403fb4af3baf2043c56be4559",
            "be9b45357917fb3dd279b2fb1d0f6df527ca1a3403fb4af3baf2043c56be4559",
            "lock_mismatch:target_diverged:x86_64-unknown-linux-gnu",
        ),
        (
            "macOS ARM generator digest",
            "ee13d875454f340daa73812a456c9624b755a2a8267bf923d81e97cc467e2b38",
            "fe13d875454f340daa73812a456c9624b755a2a8267bf923d81e97cc467e2b38",
            "lock_mismatch:target_diverged:aarch64-apple-darwin",
        ),
        (
            "macOS Intel generator digest",
            "6f9c6706b959c708e3c9804d5eed48c811dacf2e1b7afe241289ad27c4c546c5",
            "7f9c6706b959c708e3c9804d5eed48c811dacf2e1b7afe241289ad27c4c546c5",
            "lock_mismatch:target_diverged:x86_64-apple-darwin",
        ),
        (
            "Mise URL at the pinned version",
            mise_url.as_str(),
            "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-linux-x64",
            "lock_mismatch:mise_bootstrap_artifact",
        ),
        (
            "Mise digest at the pinned version",
            velnor_actions_mise::MISE_BINARY_SHA256_LINUX_X64,
            "4162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
            "lock_mismatch:mise_bootstrap_sha256",
        ),
        (
            "stale Mise version",
            version_line.as_str(),
            "version = \"2026.10.18\"",
            stale_problem.as_str(),
        ),
    ];
    for (name, from, to, expected_problem) in mutations {
        assert!(original.contains(from), "fixture lacks {name} source value");
        let mutated = original.replace(from, to);
        std::fs::write(lock_path, mutated)?;
        let error = velnor_actions_orchestrator::prepare(root)
            .and_then(|preparation| finalized_jobs(&preparation))
            .err()
            .ok_or_else(|| format!("{name} was accepted"))?;
        assert!(
            matches!(
                &error,
                OrchestratorError::Contract { problem } if problem == expected_problem
            ),
            "{name} returned {error}"
        );
    }
    std::fs::write(lock_path, original)?;
    Ok(())
}

fn current_mise_mutation_values() -> (String, String, String) {
    let version = velnor_actions_mise::MISE_VERSION;
    (
        format!(
            "https://github.com/jdx/mise/releases/download/v{version}/mise-v{version}-linux-x64"
        ),
        format!("version = \"{version}\""),
        format!("lock_mismatch:mise_bootstrap:2026.10.18:{version}"),
    )
}
