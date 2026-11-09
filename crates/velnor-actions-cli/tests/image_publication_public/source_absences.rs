// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::{Fixture, FixtureProfile};
use std::{error::Error, fs};

#[test]
fn runner_publication_rejects_present_optional_inputs_before_registry_mutation()
-> Result<(), Box<dyn Error>> {
    for path in [
        ".cargo/config",
        ".cargo/config.toml",
        "rust-toolchain",
        "images/dind/.dockerignore",
        "images/dind/Dockerfile.dockerignore",
        "images/resource-probe/.dockerignore",
        "images/resource-probe/Dockerfile.dockerignore",
        "images/runner/ubuntu-26.04/.dockerignore",
        "images/runner/ubuntu-26.04/Dockerfile.dockerignore",
    ] {
        let fixture = Fixture::new_for(FixtureProfile::Runner)?;
        fixture.set_source_response(path, b"unexpected selected input")?;
        let foreign = fixture.scratch.join("foreign.keep");
        fs::write(&foreign, b"preserve")?;
        let registry = fixture.registry_state()?;
        let archive = fs::read(&fixture.archive)?;
        let record = fs::read(&fixture.record)?;
        let output = fixture.invoke()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {path}");
        assert!(
            stderr.contains("image_publication_source_absence"),
            "{path}: {stderr}"
        );
        assert!(fixture.last_fixture_error()?.is_none());
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, registry);
        assert_eq!(fs::read(&fixture.archive)?, archive);
        assert_eq!(fs::read(&fixture.record)?, record);
        assert_eq!(fs::read(&foreign)?, b"preserve");
    }
    Ok(())
}

#[test]
fn runner_publication_accepts_authenticated_complete_tree_without_optional_inputs()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for(FixtureProfile::Runner)?;
    let output = fixture.invoke()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(fixture.receipt.exists());
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    Ok(())
}

#[test]
fn runner_publication_rejects_non_directory_absence_ancestor() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for(FixtureProfile::Runner)?;
    fixture.set_snapshot_fault("absence_parent_non_tree")?;
    let registry = fixture.registry_state()?;
    let archive = fs::read(&fixture.archive)?;
    let record = fs::read(&fixture.record)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("image_publication_source_absence"));
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry);
    assert_eq!(fs::read(&fixture.archive)?, archive);
    assert_eq!(fs::read(&fixture.record)?, record);
    assert_eq!(fs::read(&foreign)?, b"preserve");
    Ok(())
}
