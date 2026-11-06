// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path};

use velnor_actions_orchestrator::{OrchestratorError, finalized_jobs};

pub(crate) fn assert_lock_trust_rejections(
    root: &Path,
    lock_path: &Path,
    original: &str,
) -> Result<(), Box<dyn Error>> {
    let approved = velnor_actions_mise::catalog::lock::parse_generator_lock(original)?;
    let bootstrap = &approved.mise_bootstrap;
    let mise_url = &bootstrap.artifact;
    let version_line = format!("version = \"{}\"", bootstrap.version);
    let stale_problem = format!(
        "lock_mismatch:mise_bootstrap:2026.10.18:{}",
        bootstrap.version
    );
    let mutations = [
        (
            "alternate published version",
            "0.1.74",
            "0.1.73",
            "lock_mismatch:version:0.1.73:0.1.74",
        ),
        (
            "alternate source commit",
            "c622bfb187556afb8d3ff3ca917a719e4bb7f32a",
            "d622bfb187556afb8d3ff3ca917a719e4bb7f32a",
            "lock_mismatch:commit:d622bfb187556afb8d3ff3ca917a719e4bb7f32a:c622bfb187556afb8d3ff3ca917a719e4bb7f32a",
        ),
        (
            "Linux generator digest",
            "db7480b83238f85a2e4472d26ba57ca4ee4d6822353cbdaad690b1a2b963b26f",
            "eb7480b83238f85a2e4472d26ba57ca4ee4d6822353cbdaad690b1a2b963b26f",
            "lock_mismatch:target_diverged:x86_64-unknown-linux-gnu",
        ),
        (
            "macOS ARM generator digest",
            "0a97e53bdd5faada42c0e8bedce2c54f710ada669cdd58cc918aaac80df68772",
            "1a97e53bdd5faada42c0e8bedce2c54f710ada669cdd58cc918aaac80df68772",
            "lock_mismatch:target_diverged:aarch64-apple-darwin",
        ),
        (
            "macOS Intel generator digest",
            "41d55fe769f40df114ace812a19566d5c188d99143f0a631299dc10ebed4c09c",
            "51d55fe769f40df114ace812a19566d5c188d99143f0a631299dc10ebed4c09c",
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
            bootstrap.sha256.as_str(),
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
