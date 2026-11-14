// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::FamilyFixture;
use manifest_assertions::assert_manifest_matches_fixture;
use serde_json::Value;
use std::{error::Error, fs};

#[path = "family_publication/manifest_assertions.rs"]
mod manifest_assertions;

#[test]
fn publishes_coherent_family_manifest_and_rechecks_without_registry_changes()
-> Result<(), Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    let published = fixture.invoke(false)?;
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );
    let bytes = fs::read(&fixture.manifest)?;
    assert_manifest_matches_fixture(&fixture, &bytes)?;
    let state = fixture.state()?;
    let checked = fixture.invoke(true)?;
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(fs::read(&fixture.manifest)?, bytes);
    assert_registry_unchanged(&state, &fixture.state()?)?;
    Ok(())
}

#[test]
fn rejects_stale_family_before_registry_or_manifest_writes() -> Result<(), Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    fixture.mutate(|state| state["main_sha"] = Value::String("b".repeat(40)))?;
    let rejected = fixture.invoke(false)?;
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("runner_publication_not_current_main"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(!fixture.manifest.exists());
    let state = fixture.state()?;
    for registry in state["registries"]
        .as_object()
        .ok_or("registries")?
        .values()
    {
        assert!(registry["remote"].is_null());
        assert!(registry["tag_digest"].is_null());
    }
    Ok(())
}

#[test]
fn readonly_recheck_rejects_manifest_substitution_without_overwriting_it()
-> Result<(), Box<dyn Error>> {
    let fixture = published_family()?;
    let mut manifest: Value = serde_json::from_slice(&fs::read(&fixture.manifest)?)?;
    manifest["images"]["runner"]["registry_digest"] =
        Value::String(format!("sha256:{}", "0".repeat(64)));
    let substituted = serde_json::to_vec(&manifest)?;
    fs::write(&fixture.manifest, &substituted)?;
    let before = fixture.state()?;
    let rejected = fixture.invoke(true)?;
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("runner_publication_manifest_changed"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(fs::read(&fixture.manifest)?, substituted);
    assert_registry_unchanged(&before, &fixture.state()?)?;
    Ok(())
}

#[test]
fn readonly_recheck_rejects_registry_tag_drift_without_republishing() -> Result<(), Box<dyn Error>>
{
    let fixture = published_family()?;
    let manifest = fs::read(&fixture.manifest)?;
    let published = fixture.state()?;
    let replacement = &published["registries"]["ghcr.io/tailrocks/velnor-dind"];
    let mut drift = published["registries"]["ghcr.io/tailrocks/velnor-runner"].clone();
    for (key, value) in replacement["remote"]["inventory"]
        .as_object()
        .ok_or("replacement graph")?
    {
        drift["remote"]["inventory"]
            .as_object_mut()
            .ok_or("runner graph")?
            .insert(key.clone(), value.clone());
    }
    drift["tag_digest"] = replacement["tag_digest"].clone();
    fixture.mutate(|state| {
        state["registries"]["ghcr.io/tailrocks/velnor-runner"] = drift;
    })?;
    let before = fixture.state()?;
    let rejected = fixture.invoke(true)?;
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("image_publication_registry_changed"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(fs::read(&fixture.manifest)?, manifest);
    assert_registry_unchanged(&before, &fixture.state()?)?;
    Ok(())
}

#[test]
fn failed_member_never_creates_manifest_or_publishes_later_member() -> Result<(), Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    let foreign = fixture.assets.join("foreign-canary");
    fs::write(&foreign, b"foreign artifact")?;
    fixture.mutate(|state| {
        state["registries"]["ghcr.io/tailrocks/velnor-dind"]["fail_blob_readback"] =
            Value::Bool(true);
    })?;
    let failed = fixture.invoke(false)?;
    assert!(!failed.status.success());
    assert!(!fixture.manifest.exists());
    let state = fixture.state()?;
    assert!(!state["registries"]["ghcr.io/tailrocks/velnor-runner"]["remote"].is_null());
    assert!(state["registries"]["ghcr.io/tailrocks/velnor-resource-probe"]["remote"].is_null());
    assert_eq!(fs::read(foreign)?, b"foreign artifact");
    Ok(())
}

#[test]
fn family_publication_preserves_preexisting_manifest() -> Result<(), Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    fs::write(&fixture.manifest, b"foreign manifest")?;
    let failed = fixture.invoke(false)?;
    assert!(!failed.status.success());
    assert_eq!(fs::read(&fixture.manifest)?, b"foreign manifest");
    Ok(())
}

#[test]
fn rejects_non_exact_family_lock_versions_without_mutating_inputs_or_registry()
-> Result<(), Box<dyn Error>> {
    for (field, exact) in [
        ("runner_version", "2.338.0"),
        ("docker_version", "29.9.1"),
        ("buildx_version", "0.38.0"),
        ("compose_version", "5.6.0"),
        ("node_version", "26.12.0"),
    ] {
        for malformed in ["latest", "1.2.3-beta"] {
            reject_non_exact_lock_version(field, exact, malformed)?;
        }
    }
    Ok(())
}

fn reject_non_exact_lock_version(
    field: &str,
    exact: &str,
    malformed: &str,
) -> Result<(), Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    let record_path = fixture.assets.join("runner-image-publication.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path)?)?;
    record["lock"][field] = Value::String(malformed.to_owned());
    let input = serde_json::to_vec(&record)?;
    fs::write(&record_path, &input)?;
    let rejected = fixture.invoke(false)?;
    assert!(
        !rejected.status.success(),
        "accepted {malformed} for {field} from {exact}"
    );
    assert!(!fixture.manifest.exists());
    assert_eq!(fs::read(&record_path)?, input);
    for registry in fixture.state()?["registries"]
        .as_object()
        .ok_or("registries")?
        .values()
    {
        assert!(registry["remote"].is_null());
        assert!(registry["tag_digest"].is_null());
    }
    Ok(())
}

#[test]
fn rejects_malformed_family_records_before_registry_writes() -> Result<(), Box<dyn Error>> {
    for duplicate in [false, true] {
        let fixture = FamilyFixture::new()?;
        let path = fixture.assets.join("runner-image-publication.json");
        let original = fs::read_to_string(&path)?;
        let malformed = if duplicate {
            original.replacen(
                "\"schema\":",
                "\"schema\":\"velnor.runner-image-publication.v2\",\"schema\":",
                1,
            )
        } else {
            let mut record: Value = serde_json::from_str(&original)?;
            record["source"]["unexpected"] = Value::Bool(true);
            serde_json::to_string(&record)?
        };
        assert_ne!(malformed, original);
        fixture.replace_runner_record(malformed.as_bytes())?;
        let before = fixture.state()?;
        let rejected = fixture.invoke(false)?;
        let after = fixture.state()?;
        let registries_written = after["registries"]
            .as_object()
            .ok_or("registries")?
            .values()
            .filter(|registry| !registry["remote"].is_null())
            .count();
        assert!(
            !rejected.status.success(),
            "accepted malformed family record; manifest_exists={}; registries_written={registries_written}",
            fixture.manifest.exists(),
        );
        assert!(!fixture.manifest.exists());
        assert_eq!(fs::read_to_string(&path)?, malformed);
        assert_registry_unchanged(&before, &after)?;
    }
    Ok(())
}

fn published_family() -> Result<FamilyFixture, Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    let output = fixture.invoke(false)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(fixture)
}

fn assert_registry_unchanged(before: &Value, after: &Value) -> Result<(), Box<dyn Error>> {
    for (repository, registry) in before["registries"].as_object().ok_or("registries")? {
        for field in ["tag_digest", "manifest_reference", "remote"] {
            assert_eq!(after["registries"][repository][field], registry[field]);
        }
    }
    Ok(())
}

#[test]
fn incomplete_family_assets_fail_before_any_registry_write() -> Result<(), Box<dyn Error>> {
    for name in [
        "velnor-runner-linux-amd64.tar",
        "velnor-dind-linux-amd64.tar",
        "velnor-resource-probe-linux-amd64.tar",
        "runner-image-provenance.json",
        "dind-image-provenance.json",
        "resource-probe-image-provenance.json",
        "runner-image-publication.json",
        "dind-image-publication.json",
        "resource-probe-image-publication.json",
        "resource_probe",
        "storage_init",
        "job_start_hook",
        "job_started_hook",
    ] {
        reject_invalid_asset(name, "missing")?;
    }
    for case in ["empty", "directory", "symlink"] {
        reject_invalid_asset("velnor-dind-linux-amd64.tar", case)?;
    }
    Ok(())
}

fn reject_invalid_asset(name: &str, case: &str) -> Result<(), Box<dyn Error>> {
    let fixture = FamilyFixture::new()?;
    let path = fixture.assets.join(name);
    fs::remove_file(&path)?;
    let foreign = fixture.assets.join("foreign-canary");
    fs::write(&foreign, b"preserve foreign bytes")?;
    match case {
        "missing" => {}
        "empty" => fs::write(&path, b"")?,
        "directory" => fs::create_dir(&path)?,
        "symlink" => std::os::unix::fs::symlink(&foreign, &path)?,
        _ => return Err("unknown asset case".into()),
    }
    let rejected = fixture.invoke(false)?;
    assert!(!rejected.status.success(), "accepted {name} {case}");
    assert!(!fixture.manifest.exists());
    let state = fixture.state()?;
    for registry in state["registries"]
        .as_object()
        .ok_or("registries")?
        .values()
    {
        assert!(
            registry["remote"].is_null(),
            "wrote registry for {name} {case}"
        );
        assert!(registry["tag_digest"].is_null());
    }
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("runner_publication_asset_inventory"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(fs::read(&foreign)?, b"preserve foreign bytes");
    Ok(())
}
