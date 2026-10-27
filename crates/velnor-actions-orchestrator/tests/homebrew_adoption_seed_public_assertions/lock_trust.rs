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
            "0.1.59",
            "0.1.58",
            "lock_mismatch:version:0.1.58:0.1.59",
        ),
        (
            "alternate source commit",
            "4bb724119971fe49f07b12876c54c3c159edb5e5",
            "5bb724119971fe49f07b12876c54c3c159edb5e5",
            "lock_mismatch:commit:5bb724119971fe49f07b12876c54c3c159edb5e5:4bb724119971fe49f07b12876c54c3c159edb5e5",
        ),
        (
            "Linux generator digest",
            "717be00d245f511bd5d324574cb80b72a34c5a83c639fafa6af1c0acb8649b26",
            "817be00d245f511bd5d324574cb80b72a34c5a83c639fafa6af1c0acb8649b26",
            "lock_mismatch:target_diverged:x86_64-unknown-linux-gnu",
        ),
        (
            "macOS ARM generator digest",
            "36c47f77001e12c3ea80f4a2559d1b11700935c94488044a966735175b342667",
            "46c47f77001e12c3ea80f4a2559d1b11700935c94488044a966735175b342667",
            "lock_mismatch:target_diverged:aarch64-apple-darwin",
        ),
        (
            "macOS Intel generator digest",
            "20ad2cb971473595d730dd3eb07d8b6bf919f759ffbba2c3b54dca9222a80f7b",
            "30ad2cb971473595d730dd3eb07d8b6bf919f759ffbba2c3b54dca9222a80f7b",
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
