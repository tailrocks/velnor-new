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
    write_fixture_source(root, RECORD_FILE, RECORD_SOURCE)?;
    write_fixture_source(root, OCI_FILE, OCI_SOURCE)?;
    write_fixture_source(root, TAR_FILE, TAR_SOURCE)?;
    let driver = root.join("gc1_t47_driver.py");
    std::fs::write(&driver, DRIVER)?;
    for name in [
        IDENTITY_FILE,
        REGISTRY_FILE,
        PUBLISH_FILE,
        RECORD_FILE,
        OCI_FILE,
        TAR_FILE,
    ] {
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
    operand: &str,
) -> Result<(i32, String), Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(&fixture.driver)
        .arg(fixture.root.path())
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

pub(crate) fn run_envelope(
    fixture: &Fixture,
    mode: &str,
    envelope: &str,
    stem: &str,
) -> Result<(i32, String), Box<dyn Error>> {
    let path = fixture.root.path().join(format!("case-{stem}.json"));
    std::fs::write(&path, envelope)?;
    let path_text = path
        .to_str()
        .ok_or_else(|| fail("case path left its string shape"))?;
    run_mode(fixture, mode, path_text)
}

pub(crate) fn expect_ok(
    fixture: &Fixture,
    mode: &str,
    envelope: &str,
    stem: &str,
    marker: &str,
) -> Result<String, Box<dyn Error>> {
    let (code, output) = run_envelope(fixture, mode, envelope, stem)?;
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
    let (code, output) = run_envelope(fixture, mode, envelope, stem)?;
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
