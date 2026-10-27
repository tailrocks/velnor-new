// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn write_fixture_source(
    root: &Path,
    name: &str,
    source: &str,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(root.join(name), source)?;
    Ok(())
}

pub(crate) fn stage_fixed_inputs(root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    write_fixture_source(root, IDENTITY_FILE, IDENTITY_SOURCE)?;
    write_fixture_source(root, REGISTRY_FILE, REGISTRY_SOURCE)?;
    write_fixture_source(root, PUBLISH_FILE, PUBLISH_SOURCE)?;
    let driver = root.join("gc1_t46_driver.py");
    std::fs::write(&driver, [DRIVER_A, DRIVER_B].concat())?;
    for name in [IDENTITY_FILE, REGISTRY_FILE, PUBLISH_FILE] {
        let output = Command::new("python3")
            .arg("-m")
            .arg("py_compile")
            .arg(root.join(name))
            .output()?;
        if !output.status.success() {
            return Err(fail(&format!(
                "{name} left compilable:{:?}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    Ok(driver)
}

pub(crate) fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let driver = stage_fixed_inputs(root.path())?;
    Ok(Fixture { root, driver })
}

pub(crate) fn run_mode(
    fixture: &Fixture,
    mode: &str,
    envelope: &str,
    stem: &str,
) -> Result<(i32, String), Box<dyn Error>> {
    let path = fixture.root.path().join(format!("case-{stem}.json"));
    std::fs::write(&path, envelope)?;
    let output = Command::new("python3")
        .arg(&fixture.driver)
        .arg(fixture.root.path())
        .arg(mode)
        .arg(&path)
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("fixed validator left no exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

pub(crate) fn expect_ok(
    fixture: &Fixture,
    mode: &str,
    envelope: &str,
    stem: &str,
    marker: &str,
) -> Result<String, Box<dyn Error>> {
    let (code, output) = run_mode(fixture, mode, envelope, stem)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed logic rejected its valid input:{mode}:{output}"
        )));
    }
    require(&output, marker, mode)?;
    Ok(output)
}

pub(crate) fn expect_rejection(
    fixture: &Fixture,
    mode: &str,
    envelope: &str,
    stem: &str,
    marker: &str,
    message: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let (code, output) = run_mode(fixture, mode, envelope, stem)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed logic left its rejection:{mode}:{output}"
        )));
    }
    require(&output, marker, stem)?;
    if let Some(expected) = message {
        require(&output, expected, stem)?;
    }
    Ok(())
}

pub(crate) fn tar_entry(name: &str, data: &[u8], typeflag: u8) -> Vec<u8> {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    let size_field = format!("{:011o}\0", data.len());
    header[124..136].copy_from_slice(size_field.as_bytes());
    header[148..156].copy_from_slice(b"        ");
    header[156] = typeflag;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let checksum_field = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_field.as_bytes());
    let mut entry = header.to_vec();
    entry.extend_from_slice(data);
    while entry.len() % 512 != 0 {
        entry.push(0);
    }
    entry
}

pub(crate) fn tar_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, data) in entries {
        archive.extend_from_slice(&tar_entry(name, data, b'0'));
    }
    archive.extend_from_slice(&[0u8; 1024]);
    archive
}

pub(crate) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) const WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main";

pub(crate) fn identity_envelope() -> String {
    format!(
        concat!(
            "{{\"repository\":\"tailrocks/velnor-new\",\"repository_id\":1390620900,",
            "\"event\":\"workflow_dispatch\",\"workflow_ref\":\"{workflow_ref}\",",
            "\"ref\":\"refs/heads/main\",\"authority_commit\":\"{source}\",",
            "\"run_id\":7,\"run_attempt\":2,\"source\":\"{source}\",\"artifact_id\":9}}"
        ),
        workflow_ref = WORKFLOW_REF,
        source = SOURCE_SHA,
    )
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
