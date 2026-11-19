// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::lock::locked;
use crate::pipeline::workflow;

const BATCHED: [&str; 6] = [
    "restore-all",
    "--scope-cache",
    "--roots-env",
    "--targets-env",
    "--target-env",
    "--paths-env",
];

fn two_roots(version: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = locked(version)?;
    let more = crate::pipeline::view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    repo.rust_metadata = more.rust_metadata;
    repo.tracked_paths.extend(more.tracked_paths);
    let path = RepoPath::new(".velnor/rust-ci.toml")?;
    repo.tracked_paths.insert(path.clone());
    repo.files.insert(path, config.as_bytes().to_vec());
    workflow(&repo)
}

#[test]
fn the_released_helper_gets_only_the_commands_it_has() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        let text = two_roots("0.1.81", &format!("shape = '{shape}'\n"))?;
        for batched in BATCHED {
            assert!(
                !text.contains(batched),
                "{shape}: {batched} in 0.1.81 output"
            );
        }
        assert!(!text.contains("inputs_save"), "{shape}");
        assert!(
            text.contains("inputs restore --target-dir target"),
            "{shape}"
        );
        assert!(text.contains("inputs save --target-dir"), "{shape}");
        assert!(text.contains("VELNOR_CACHE_TARGETS: |"), "{shape}");
        assert!(text.contains("cache decide --target-dir"), "{shape}");
    }
    Ok(())
}

#[test]
fn a_helper_with_the_batched_commands_gets_them() -> TestResult {
    for version in ["0.1.97", "0.1.100"] {
        let text = two_roots(version, "")?;
        for batched in ["restore-all", "--scope-cache"] {
            assert!(text.contains(batched), "{version}: {batched}");
        }
        assert!(!text.contains("VELNOR_CACHE_TARGETS"), "{version}");
        assert!(text.contains("id: inputs_save"), "{version}");
    }
    Ok(())
}

#[test]
fn a_helper_released_before_the_batched_commands_does_not_get_them() -> TestResult {
    for version in ["0.1.95", "0.1.96"] {
        let text = two_roots(version, "")?;
        for batched in BATCHED {
            assert!(!text.contains(batched), "{version}: {batched}");
        }
        assert!(text.contains("VELNOR_CACHE_TARGETS: |"), "{version}");
        assert!(
            text.contains("inputs restore --target-dir target"),
            "{version}"
        );
    }
    Ok(())
}
