// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_accepts_valid_archive(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    let path = write_bytes(fixture.root.path(), "candidate.oci.tar", archive)?;
    let (code, output) = run_driver(&fixture.driver, fixture.root.path(), "archive_ok", &path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed oci validator rejected its valid archive:{output}"
        )));
    }
    require(&output, "ARCHIVE-OK", "oci acceptance")?;
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    for expected in [
        format!("archive_index_digest={}", sha256_hex(&parts.index)),
        format!("archive_index_size_bytes={}", parts.index.len()),
        format!("platform_manifest_digest=sha256:{manifest_hex}"),
        format!("platform_manifest_size_bytes={}", parts.manifest.len()),
        format!("image_configuration_digest=sha256:{config_hex}"),
        format!("image_configuration_size_bytes={}", parts.config.len()),
        format!("layer_digests=sha256:{layer_hex}"),
    ] {
        require(&output, &expected, "oci identity")?;
    }
    Ok(())
}

pub(crate) fn check_accepts_valid_record(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    let record = record_json(parts, archive);
    let path = write_bytes(fixture.root.path(), "candidate.json", record.as_bytes())?;
    let (code, output) = run_driver(&fixture.driver, fixture.root.path(), "record_ok", &path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed record validator rejected its valid record:{output}"
        )));
    }
    require(&output, "RECORD-OK", "record acceptance")?;
    Ok(())
}

pub(crate) fn check_accepts_safe_paths(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    for (mode, name, marker) in [
        ("tar_member", "oci-layout", "TAR-MEMBER-OK oci-layout"),
        (
            "tar_member",
            "blobs/sha256/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "TAR-MEMBER-OK blobs/sha256/",
        ),
        ("tar_member", "a/b/c", "TAR-MEMBER-OK a/b/c"),
        ("tar_layer", "a/b", "TAR-LAYER-OK a/b"),
        ("tar_layer", "", "TAR-LAYER-OK ."),
        ("tar_layer", ".", "TAR-LAYER-OK ."),
    ] {
        let (code, output) = run_driver(&fixture.driver, fixture.root.path(), mode, name)?;
        if code != 0 {
            return Err(fail(&format!(
                "fixed tar validator rejected its safe path:{name}:{output}"
            )));
        }
        require(&output, marker, "tar acceptance")?;
    }
    Ok(())
}

pub(crate) fn expect_record_rejection(
    fixture: &Fixture,
    name: &str,
    record: &str,
    message: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let path = write_bytes(fixture.root.path(), name, record.as_bytes())?;
    let (code, output) = run_driver(&fixture.driver, fixture.root.path(), "record_bad", &path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed record validator left its rejection:{name}:{output}"
        )));
    }
    require(&output, "RECORD-REJECTED", name)?;
    if let Some(expected) = message {
        require(&output, expected, name)?;
    }
    Ok(())
}

pub(crate) fn expect_archive_rejection(
    fixture: &Fixture,
    name: &str,
    archive: &[u8],
    message: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let path = write_bytes(fixture.root.path(), name, archive)?;
    let (code, output) = run_driver(&fixture.driver, fixture.root.path(), "archive_bad", &path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed oci validator left its rejection:{name}:{output}"
        )));
    }
    require(&output, "ARCHIVE-REJECTED", name)?;
    if let Some(expected) = message {
        require(&output, expected, name)?;
    }
    Ok(())
}

pub(crate) fn expect_tar_rejection(
    fixture: &Fixture,
    mode: &str,
    name: &str,
    marker: &str,
    message: &str,
) -> Result<(), Box<dyn Error>> {
    let (code, output) = run_driver(&fixture.driver, fixture.root.path(), mode, name)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed tar validator left its rejection:{name}:{output}"
        )));
    }
    require(&output, marker, name)?;
    require(&output, message, name)?;
    Ok(())
}

pub(crate) fn replace_once(
    haystack: &str,
    needle: &str,
    replacement: &str,
) -> Result<String, Box<dyn Error>> {
    if !haystack.contains(needle) {
        return Err(fail(&format!("fixture lost its mutation point:{needle}")));
    }
    Ok(haystack.replacen(needle, replacement, 1))
}

#[test]
fn gc1_t45_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_sources_are_fixed()?;
    let (fixture, parts, archive) = fixture()?;
    check_accepts_valid_archive(&fixture, &parts, &archive)?;
    check_accepts_valid_record(&fixture, &parts, &archive)?;
    check_accepts_safe_paths(&fixture)?;
    Ok(())
}
