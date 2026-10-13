use std::{error::Error, fs, os::unix::fs::symlink};

use super::{Fixture, assert_disabled, payload, run_and_read, run_and_read_removed};

#[test]
fn hosted_identity_disables_on_image_arch_or_version_mismatch() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    for (key, value, reason) in [
        ("ImageOS", "ubuntu24", "image_os_mismatch"),
        ("RUNNER_ARCH", "ARM64", "runner_arch_mismatch"),
        ("ImageVersion", "", "image_version_missing"),
    ] {
        let (output, text) = run_and_read(&hosted, &fixture, &[(key, value.to_owned())]);
        assert_disabled(&output, &text, reason);
    }
    for (variable, reason) in [
        ("RUNNER_OS", "runner_os_missing"),
        ("RUNNER_ARCH", "runner_arch_missing"),
        ("ImageOS", "image_os_missing"),
        ("ImageVersion", "image_version_missing"),
    ] {
        let (output, text) = run_and_read_removed(&hosted, &fixture, &[variable]);
        assert_disabled(&output, &text, reason);
    }
    let (output, text) = run_and_read(&payload("ubuntu-26.04-arm"), &fixture, &[]);
    assert_disabled(&output, &text, "lane_image_unqualified");
    Ok(())
}

#[test]
fn hosted_identity_disables_for_unarchived_or_aliased_mise_roots() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    let alternative = fixture.root.join("mise elsewhere");
    fs::create_dir_all(&alternative)?;
    let (output, text) = run_and_read(
        &hosted,
        &fixture,
        &[("MISE_DATA_DIR", alternative.to_string_lossy().into_owned())],
    );
    assert_disabled(&output, &text, "mise_root_not_archived");

    let xdg = fixture.root.join("xdg data");
    fs::create_dir_all(&xdg)?;
    let (output, text) = run_and_read(
        &hosted,
        &fixture,
        &[("XDG_DATA_HOME", xdg.to_string_lossy().into_owned())],
    );
    assert_disabled(&output, &text, "mise_root_not_archived");

    let actual = fixture.home.join("mise-store");
    fs::create_dir_all(&actual)?;
    symlink(&actual, fixture.home.join(".local/share/mise"))?;
    let (output, text) = run_and_read(&hosted, &fixture, &[]);
    assert_disabled(&output, &text, "mise_root_aliased");
    Ok(())
}

#[test]
fn hosted_identity_disables_for_invalid_native_or_mise_homes() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    let mismatch = fixture.root.join("other cargo");
    let (output, text) = run_and_read(
        &hosted,
        &fixture,
        &[("CARGO_HOME", mismatch.to_string_lossy().into_owned())],
    );
    assert_disabled(&output, &text, "cargo_home_mismatch");

    let (output, text) = run_and_read(
        &hosted,
        &fixture,
        &[("RUSTUP_HOME", "relative/rustup".to_owned())],
    );
    assert_disabled(&output, &text, "rustup_home_mismatch");

    let (output, text) = run_and_read_removed(&hosted, &fixture, &["MISE_RUSTUP_HOME"]);
    assert_disabled(&output, &text, "mise_rustup_home_mismatch");
    Ok(())
}

#[test]
fn hosted_identity_disables_for_root_home_or_runner_temp() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    for (key, reason) in [
        ("HOME", "home_aliased"),
        ("RUNNER_TEMP", "runner_temp_aliased"),
    ] {
        let (output, text) = run_and_read(&hosted, &fixture, &[(key, "/".to_owned())]);
        assert_disabled(&output, &text, reason);
    }
    Ok(())
}
