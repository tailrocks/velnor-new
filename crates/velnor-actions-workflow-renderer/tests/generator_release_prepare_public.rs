// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

#[path = "generator_release_prepare_fixture.rs"]
mod generator_release_prepare_fixture;
use generator_release_prepare_fixture::{Fixture, Scenario, output_text};

#[test]
fn existing_generator_release_completes_without_checkout_assets() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(Scenario::Valid)?;
    let output = fixture.execute()?;
    assert!(output.status.success(), "{}", output_text(&output));
    assert_eq!(
        fs::read_to_string(&fixture.github_output)?,
        "action=complete\n"
    );
    assert!(!fixture.empty_workspace.join("linux-assets").exists());
    assert!(!fixture.empty_workspace.join("manifest-assets").exists());
    Ok(())
}

#[test]
fn generator_release_rejects_corrupt_binary_without_completion() -> Result<(), Box<dyn Error>> {
    assert_rejected(Scenario::CorruptBinary)
}

#[test]
fn generator_release_rejects_mismatched_manifest_without_completion() -> Result<(), Box<dyn Error>>
{
    assert_rejected(Scenario::MismatchedManifest)
}

#[test]
fn generator_release_rejects_wrong_provenance_source_without_completion()
-> Result<(), Box<dyn Error>> {
    assert_rejected(Scenario::WrongProvenanceSource)
}

#[test]
fn generator_release_rejects_wrong_provenance_toolchain_without_completion()
-> Result<(), Box<dyn Error>> {
    assert_rejected(Scenario::WrongProvenanceToolchain)
}

#[test]
fn generator_release_rejects_attestation_refusal_without_completion() -> Result<(), Box<dyn Error>>
{
    assert_rejected(Scenario::AttestationRefused)
}

#[test]
fn generator_release_rejects_partial_attestation_refusal_without_completion()
-> Result<(), Box<dyn Error>> {
    assert_rejected(Scenario::PartialAttestationRefused)
}

fn assert_rejected(scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(scenario)?;
    let output = fixture.execute()?;
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        output_text(&output)
    );
    let completion = fs::read_to_string(&fixture.github_output)?;
    assert!(
        !completion.contains("action=complete"),
        "release completed after rejection"
    );
    Ok(())
}
