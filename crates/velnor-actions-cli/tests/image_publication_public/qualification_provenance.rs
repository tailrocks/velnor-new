// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::support::{Fixture, FixtureProfile, RejectionCase};

#[test]
fn signed_qualification_binds_envelope_and_context_for_every_role() -> Result<(), Box<dyn Error>> {
    for profile in [
        FixtureProfile::Runner,
        FixtureProfile::Dind,
        FixtureProfile::ResourceProbe,
    ] {
        for case in [
            RejectionCase::WrongProofEnvelopeDigest,
            RejectionCase::WrongProofContextDigest,
        ] {
            reject_qualification_provenance(case, profile)?;
        }
    }
    Ok(())
}

fn reject_qualification_provenance(
    case: RejectionCase,
    profile: FixtureProfile,
) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for(profile)?;
    fixture.apply_rejection(case)?;
    let proof_path = fixture
        .qualification_proof
        .as_ref()
        .ok_or("qualification receipt")?;
    let proof_bytes = fs::read(proof_path)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let archive = fs::read(&fixture.archive)?;
    let record = fs::read(&fixture.record)?;
    let assets = fixture.archive.parent().ok_or("family assets")?;
    let mut names = vec![profile.envelope_name()];
    if profile == FixtureProfile::Runner {
        names.extend([
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ]);
    } else {
        names.push("resource_probe");
    }
    let snapshots = names
        .iter()
        .map(|name| Ok((assets.join(name), fs::read(assets.join(name))?)))
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let registry = fixture.preseed_registry_state()?;
    let output = fixture.invoke()?;
    let fixture_error = fixture.last_fixture_error()?;
    assert!(
        !output.status.success(),
        "accepted {case:?} for {}",
        profile.name()
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("image_qualification_proof_candidate"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry);
    assert_eq!(fs::read(&fixture.archive)?, archive);
    assert_eq!(fs::read(&fixture.record)?, record);
    assert_eq!(fs::read(proof_path)?, proof_bytes);
    for (path, bytes) in snapshots {
        assert_eq!(fs::read(path)?, bytes);
    }
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    super::assert_fixture_result(case, fixture_error.as_deref());
    Ok(())
}
