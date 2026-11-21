// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::release_family_fixture::generate;
use crate::release_fixture::{APP, LOCK, release_view};
use crate::step_refs::{dangling_refs, expressions, gate_patches, step_refs, strings};
use crate::yaml_tree::document;

const RELEASE_GATES: &[(u64, &str)] = &[
    (
        82,
        "pr_only_checks = [\"DCO\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        86,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nprofile = \"release\"\n",
    ),
    (
        88,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nformat = \"raw\"\n",
    ),
    (
        90,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nbins = [{ bin = \"rg\" }]\ninclude = [\"LICENSE\"]\n",
    ),
    (
        90,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nasset_name = \"{{ bin }}_{{ platform }}\"\n",
    ),
    (
        90,
        "[[checks]]\nname = \"Lint the repository\"\nrun = \"bash scripts/lint.sh\"\nruns_on = \"ubuntu-24.04\"\ntimeout_minutes = 10\n[checks.permissions]\nactions = \"read\"\n",
    ),
    (
        90,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[checks]]\nname = \"Qualify linux\"\nrun = \"bash scripts/qualify.sh\"\nbefore = \"publish\"\nartifacts = [\"app-linux-x64\"]\n",
    ),
    (
        90,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[checks]]\nname = \"one\"\nrun = \"bash a.sh\"\n[[checks]]\nname = \"two\"\nrun = \"bash b.sh\"\nneeds = [\"one\"]\n",
    ),
    (
        91,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nattest = true\n",
    ),
    (
        92,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nsmoke = true\ntarget = \"gnu\"\ncommit_file = true\nchecksum_files = \"sha256\"\n",
    ),
    (
        93,
        "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nattest = true\nmember_digests = \"sha256\"\nbins = [{ bin = \"rg\" }]\n",
    ),
];

const FAMILY_GATES: &[(u64, &str)] = &[
    (
        85,
        "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        98,
        "family_tag = \"tools-v{{ version }}\"\nfamily_trigger = \"tag_push\"\nchecksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        99,
        "family_tag = \"tools-v{{ version }}\"\nfamily_trigger = \"tag_push\"\nfamily_required_checks = true\nroots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n",
    ),
    (
        110,
        "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\nchecksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n",
    ),
];

fn check(text: &str, config: &str, patch: u64) -> Result<usize, Box<dyn Error>> {
    let yaml = document(text)?;
    let mut texts = Vec::new();
    strings(&yaml, &mut texts);
    let refs = texts
        .iter()
        .flat_map(|text| expressions(text))
        .map(|expr| step_refs(expr).len())
        .sum();
    let dangling = dangling_refs(&yaml);
    assert!(
        dangling.is_empty(),
        "helper 0.1.{patch}, {config:?}: {dangling:#?}"
    );
    Ok(refs)
}

#[test]
fn every_release_gate_renders_steps_that_name_their_own_job() -> TestResult {
    let patches = gate_patches()?;
    let mut refs = 0;
    for (threshold, config) in RELEASE_GATES {
        for patch in patches.iter().copied().filter(|patch| *patch >= *threshold) {
            let mut view = release_view(&[APP], Some(config))?;
            view.files.insert(
                LOCK_PATH,
                LOCK.replace("0.1.81", &format!("0.1.{patch}")).into_bytes(),
            );
            let files = expected_files(&view)
                .map_err(|error| format!("{config:?} at 0.1.{patch}: {error}"))?;
            for text in files.values() {
                refs += check(text, config, patch)?;
            }
        }
    }
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}

#[test]
fn every_family_gate_renders_steps_that_name_their_own_job() -> TestResult {
    let patches = gate_patches()?;
    let mut refs = 0;
    for (threshold, config) in FAMILY_GATES {
        for patch in patches.iter().copied().filter(|patch| *patch >= *threshold) {
            let (text, _) = generate(&format!("0.1.{patch}"), config)
                .map_err(|error| format!("{config:?} at 0.1.{patch}: {error}"))?;
            refs += check(&text, config, patch)?;
        }
    }
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
}
