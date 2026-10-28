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
            "0.1.66",
            "0.1.65",
            "lock_mismatch:version:0.1.65:0.1.66",
        ),
        (
            "alternate source commit",
            "ecee0521d8631315790662107231dabf2ba6b1ec",
            "fcee0521d8631315790662107231dabf2ba6b1ec",
            "lock_mismatch:commit:fcee0521d8631315790662107231dabf2ba6b1ec:ecee0521d8631315790662107231dabf2ba6b1ec",
        ),
        (
            "Linux generator digest",
            "fe7cc33aae28b9be2fe9a0561e148c666516f30e53d65a069a6f3f5575a53298",
            "ee7cc33aae28b9be2fe9a0561e148c666516f30e53d65a069a6f3f5575a53298",
            "lock_mismatch:target_diverged:x86_64-unknown-linux-gnu",
        ),
        (
            "macOS ARM generator digest",
            "a319646c4c92bfdfc9abd623883338c2d18ef7e5549e00633dcba178fdd45e9d",
            "b319646c4c92bfdfc9abd623883338c2d18ef7e5549e00633dcba178fdd45e9d",
            "lock_mismatch:target_diverged:aarch64-apple-darwin",
        ),
        (
            "macOS Intel generator digest",
            "8193c279607c971da1d599e53b8ba6a1c87ba01f4ddc9c2fe993f8512ef852cf",
            "9193c279607c971da1d599e53b8ba6a1c87ba01f4ddc9c2fe993f8512ef852cf",
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
