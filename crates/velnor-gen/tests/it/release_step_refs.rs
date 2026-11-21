// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::thread;

use velnor_gen::expected_files;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::release_family_fixture::generate;
use crate::release_fixture::{APP, LOCK, release_view};
use crate::step_refs::{Callees, dangling_refs};
use crate::step_refs_sweep::{gate_patch, gate_patches};
use crate::yaml_tree::document;

const RELEASE_GATES: &[(&[&str], &str)] = &[
    (
        &["pr_only_checks", "binaries.package", "binaries.platforms"],
        "pr_only_checks = [\"DCO\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        &["binaries.profile"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nprofile = \"release\"\n",
    ),
    (
        &["binaries.format"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nformat = \"raw\"\n",
    ),
    (
        &["binaries.bins", "binaries.include"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nbins = [{ bin = \"rg\" }]\ninclude = [\"LICENSE\"]\n",
    ),
    (
        &["binaries.asset_name"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nasset_name = \"{{ bin }}_{{ platform }}\"\n",
    ),
    (
        &[
            "checks",
            "checks.name",
            "checks.run",
            "checks.runs_on",
            "checks.timeout_minutes",
            "checks.permissions",
        ],
        "[[checks]]\nname = \"Lint the repository\"\nrun = \"bash scripts/lint.sh\"\nruns_on = \"ubuntu-24.04\"\ntimeout_minutes = 10\n[checks.permissions]\nactions = \"read\"\n",
    ),
    (
        &["checks.artifacts", "checks.before"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[checks]]\nname = \"Qualify linux\"\nrun = \"bash scripts/qualify.sh\"\nbefore = \"publish\"\nartifacts = [\"app-linux-x64\"]\n",
    ),
    (
        &["checks.needs"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[checks]]\nname = \"one\"\nrun = \"bash a.sh\"\n[[checks]]\nname = \"two\"\nrun = \"bash b.sh\"\nneeds = [\"one\"]\n",
    ),
    (
        &["binaries.attest"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nattest = true\n",
    ),
    (
        &[
            "binaries.smoke",
            "binaries.target",
            "binaries.commit_file",
            "binaries.checksum_files",
        ],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nsmoke = true\ntarget = \"gnu\"\ncommit_file = true\nchecksum_files = \"sha256\"\n",
    ),
    (
        &["binaries.member_digests", "binaries.bins"],
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nattest = true\nmember_digests = \"sha256\"\nbins = [{ bin = \"rg\" }]\n",
    ),
];

const FAMILY_GATES: &[(&[&str], &str)] = &[
    (
        &["family_tag", "roots"],
        "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        &["family_trigger", "checksums_file", "commit_file", "attest"],
        "family_tag = \"tools-v{{ version }}\"\nfamily_trigger = \"tag_push\"\nchecksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        &["family_required_checks"],
        "family_tag = \"tools-v{{ version }}\"\nfamily_trigger = \"tag_push\"\nfamily_required_checks = true\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        &["binaries.optional_platforms"],
        "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\nchecksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n",
    ),
];

fn threshold(keys: &[&str]) -> Result<u64, Box<dyn Error>> {
    let mut gate = 0;
    for key in keys {
        gate = gate.max(gate_patch(key)?);
    }
    Ok(gate)
}

fn check(text: &str, config: &str, patch: u64) -> Result<usize, Box<dyn Error>> {
    let yaml = document(text)?;
    let (refs, dangling) = dangling_refs(&yaml, &Callees::new());
    assert!(
        dangling.is_empty(),
        "helper 0.1.{patch}, {config:?}: {dangling:#?}"
    );
    Ok(refs)
}

fn sweep_release_gate(
    keys: &[&str],
    config: &str,
    patches: &BTreeSet<u64>,
) -> Result<usize, Box<dyn Error>> {
    let gate = threshold(keys)?;
    let mut refs = 0;
    for patch in patches.iter().copied().filter(|patch| *patch >= gate) {
        let mut view = release_view(&[APP], Some(config))?;
        view.files.insert(
            LOCK_PATH,
            LOCK.replace("0.1.81", &format!("0.1.{patch}")).into_bytes(),
        );
        let files =
            expected_files(&view).map_err(|error| format!("{config:?} at 0.1.{patch}: {error}"))?;
        for text in files.values() {
            refs += check(text, config, patch)?;
        }
    }
    Ok(refs)
}

#[test]
fn every_release_gate_renders_steps_that_name_their_own_job() -> TestResult {
    let patches = gate_patches()?;
    let results: Vec<Result<usize, String>> = thread::scope(|scope| {
        let handles: Vec<_> = RELEASE_GATES
            .iter()
            .map(|(keys, config)| {
                let patches = &patches;
                scope.spawn(move || {
                    sweep_release_gate(keys, config, patches).map_err(|error| error.to_string())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("a release gate sweep panicked".to_owned()))
            })
            .collect()
    });
    let mut refs = 0;
    for result in results {
        refs += result?;
    }
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}

#[test]
fn every_family_gate_renders_steps_that_name_their_own_job() -> TestResult {
    let patches = gate_patches()?;
    let mut refs = 0;
    for (keys, config) in FAMILY_GATES {
        let gate = threshold(keys)?;
        for patch in patches.iter().copied().filter(|patch| *patch >= gate) {
            let (text, _) = generate(&format!("0.1.{patch}"), config)
                .map_err(|error| format!("{config:?} at 0.1.{patch}: {error}"))?;
            refs += check(&text, config, patch)?;
        }
    }
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}
