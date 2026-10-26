// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) driver: std::path::PathBuf,
}

pub(crate) fn write_fixture_source(
    root: &Path,
    name: &str,
    source: &str,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(root.join(name), source)?;
    Ok(())
}

pub(crate) fn stage_fixed_inputs(root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    write_fixture_source(root, RECORD_FILE, RECORD_SOURCE)?;
    write_fixture_source(root, OCI_FILE, OCI_SOURCE)?;
    write_fixture_source(root, TAR_FILE, TAR_SOURCE)?;
    let driver = root.join("gc1_t45_driver.py");
    std::fs::write(&driver, DRIVER)?;
    for name in [RECORD_FILE, OCI_FILE, TAR_FILE] {
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

pub(crate) fn run_driver(
    driver: &Path,
    root: &Path,
    mode: &str,
    operand: &str,
) -> Result<(i32, String), Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(driver)
        .arg(root)
        .arg(mode)
        .arg(operand)
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("fixed validator left no exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
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

pub(crate) struct ImageParts {
    pub(crate) layer: Vec<u8>,
    pub(crate) config: Vec<u8>,
    pub(crate) manifest: Vec<u8>,
    pub(crate) index: Vec<u8>,
    pub(crate) layout: Vec<u8>,
}
