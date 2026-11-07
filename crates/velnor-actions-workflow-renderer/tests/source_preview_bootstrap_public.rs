// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path, process::Command};

#[test]
fn source_preview_requires_successful_exact_source_jobs() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("tests/source_preview_source_scenarios.py"))
        .arg(root.join("src/schema2_generator_source_preview_source.py"))
        .output()?;
    assert!(
        output.status.success(),
        "source preview accepted an unqualified source: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn protected_base_controls_preview_acquisition() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("tests/source_preview_bootstrap_scenarios.py"))
        .arg(root.join("src/native_helper/source_preview_bootstrap.py"))
        .output()?;
    assert!(
        output.status.success(),
        "preview acquisition did not enforce the protected base: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn source_preview_draft_requires_exact_source_and_payload() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("tests/source_preview_publication_scenarios.py"))
        .arg(root.join("src/schema2_generator_source_preview_publication.py"))
        .output()?;
    assert!(
        output.status.success(),
        "preview draft admitted untrusted data: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn source_preview_artifact_requires_bounded_verified_elf() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("tests/source_preview_artifact_scenarios.py"))
        .arg(root.join("src/schema2_generator_source_preview_artifact.py"))
        .output()?;
    assert!(
        output.status.success(),
        "preview artifact admitted untrusted bytes: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn source_preview_tag_creation_requires_owner_and_qualified_source() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(root.join("tests/source_preview_tag_create_scenarios.py"))
        .arg(root.join("src/source_preview_tag_create.py"))
        .output()?;
    assert!(
        output.status.success(),
        "preview tag creation did not enforce its authority: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
