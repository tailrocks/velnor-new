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
            "0.1.63",
            "0.1.62",
            "lock_mismatch:version:0.1.62:0.1.63",
        ),
        (
            "alternate source commit",
            "4afd0a059ce19d03a3e88f5cdc03e7e533c20218",
            "5afd0a059ce19d03a3e88f5cdc03e7e533c20218",
            "lock_mismatch:commit:5afd0a059ce19d03a3e88f5cdc03e7e533c20218:4afd0a059ce19d03a3e88f5cdc03e7e533c20218",
        ),
        (
            "Linux generator digest",
            "4b9ab0e070a8126402d4fb2c987a7f2a24d486ca480c13388f9d3a7dc6768179",
            "5b9ab0e070a8126402d4fb2c987a7f2a24d486ca480c13388f9d3a7dc6768179",
            "lock_mismatch:target_diverged:x86_64-unknown-linux-gnu",
        ),
        (
            "macOS ARM generator digest",
            "876f5f8f2d4281946a858ea3bf0598fab13a4b5c9a27836e444a6e274d19aa13",
            "976f5f8f2d4281946a858ea3bf0598fab13a4b5c9a27836e444a6e274d19aa13",
            "lock_mismatch:target_diverged:aarch64-apple-darwin",
        ),
        (
            "macOS Intel generator digest",
            "eddb0930e40ac004a678e93af55fad700a2ec92681f913e2545ea5c634fd6cb9",
            "fddb0930e40ac004a678e93af55fad700a2ec92681f913e2545ea5c634fd6cb9",
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
