use std::{error::Error, fs};

use super::*;

#[test]
fn runtime_fingerprint_is_stable_and_binds_image_and_absolute_roots() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new()?;
    let hosted = payload("ubuntu-26.04");
    let (output, baseline) = run_and_read(&hosted, &fixture, &[]);
    assert!(output.status.success(), "{output:?}");
    let baseline_identity = identity(&baseline).to_owned();

    let (output, repeated) = run_and_read(&hosted, &fixture, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(identity(&repeated), baseline_identity);

    let (output, image_changed) = run_and_read(
        &hosted,
        &fixture,
        &[("ImageVersion", "20261005.1".to_owned())],
    );
    assert!(output.status.success(), "{output:?}");
    assert_ne!(identity(&image_changed), baseline_identity);

    let alternative_home = fixture.root.join("alternate home");
    fs::create_dir_all(alternative_home.join(".local/share"))?;
    let (output, home_changed) = run_and_read(
        &hosted,
        &fixture,
        &[("HOME", alternative_home.to_string_lossy().into_owned())],
    );
    assert!(output.status.success(), "{output:?}");
    assert_ne!(identity(&home_changed), baseline_identity);

    let alternative_temp = fixture.root.join("alternate runner temp");
    fs::create_dir_all(&alternative_temp)?;
    let rustup_home = alternative_temp.join("velnor/rustup");
    let cargo_home = alternative_temp.join("velnor/cargo");
    let overrides = [
        (
            "RUNNER_TEMP",
            alternative_temp.to_string_lossy().into_owned(),
        ),
        (
            "MISE_RUSTUP_HOME",
            rustup_home.to_string_lossy().into_owned(),
        ),
        ("RUSTUP_HOME", rustup_home.to_string_lossy().into_owned()),
        ("MISE_CARGO_HOME", cargo_home.to_string_lossy().into_owned()),
        ("CARGO_HOME", cargo_home.to_string_lossy().into_owned()),
    ];
    let (output, temp_changed) = run_and_read(&hosted, &fixture, &overrides);
    assert!(output.status.success(), "{output:?}");
    assert_ne!(identity(&temp_changed), baseline_identity);
    Ok(())
}
