// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path};

use velnor_actions_orchestrator::{OrchestratorError, finalized_jobs};

pub(crate) fn assert_lock_trust_rejections(
    root: &Path,
    lock_path: &Path,
    original: &str,
) -> Result<(), Box<dyn Error>> {
    let mutations = [
        (
            "alternate published version",
            "0.1.55",
            "0.1.54",
            "lock_mismatch:version:0.1.54:0.1.55",
        ),
        (
            "alternate source commit",
            "693f3aae14406fb7851ac70f0aee647a869b5063",
            "793f3aae14406fb7851ac70f0aee647a869b5063",
            "lock_mismatch:commit:793f3aae14406fb7851ac70f0aee647a869b5063:693f3aae14406fb7851ac70f0aee647a869b5063",
        ),
        (
            "Linux generator digest",
            "c8f1440e4f767ecc903e6611c56c2db4005011d33ee6f0fd566f4b7e5345eba4",
            "d8f1440e4f767ecc903e6611c56c2db4005011d33ee6f0fd566f4b7e5345eba4",
            "lock_mismatch:target_diverged:x86_64-unknown-linux-gnu",
        ),
        (
            "macOS ARM generator digest",
            "eb8690b6a05b18bf025a0d7876c8497cedc04d54df04fdb5a3c20d203478e756",
            "fb8690b6a05b18bf025a0d7876c8497cedc04d54df04fdb5a3c20d203478e756",
            "lock_mismatch:target_diverged:aarch64-apple-darwin",
        ),
        (
            "macOS Intel generator digest",
            "0938a574c00b05a71735285bf0d2840235ca4f9559359909564dd53b27d77883",
            "1938a574c00b05a71735285bf0d2840235ca4f9559359909564dd53b27d77883",
            "lock_mismatch:target_diverged:x86_64-apple-darwin",
        ),
        (
            "Mise URL at the pinned version",
            "https://github.com/jdx/mise/releases/download/v2026.10.19/mise-v2026.10.19-linux-x64",
            "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-linux-x64",
            "lock_mismatch:mise_bootstrap_artifact",
        ),
        (
            "Mise digest at the pinned version",
            "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
            "4162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
            "lock_mismatch:mise_bootstrap_sha256",
        ),
        (
            "stale Mise version",
            "version = \"2026.10.19\"",
            "version = \"2026.10.18\"",
            "lock_mismatch:mise_bootstrap:2026.10.18:2026.10.19",
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
