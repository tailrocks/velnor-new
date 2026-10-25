use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Output, Stdio};

use super::{GUARD, fixture_root};

fn invoke(arguments: &[&OsStr]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(GUARD)
        .args(arguments)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?)
}

fn valid_manifest() -> &'static str {
    r#"{"schema":1,"version":"0.1.52","repository":"tailrocks/velnor-new","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","targets":[{"target":"x86_64-unknown-linux-gnu","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu","sha256":"1111111111111111111111111111111111111111111111111111111111111111"},{"target":"aarch64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-aarch64-apple-darwin","sha256":"2222222222222222222222222222222222222222222222222222222222222222"},{"target":"x86_64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-apple-darwin","sha256":"3333333333333333333333333333333333333333333333333333333333333333"}]}"#
}

fn workflow() -> &'static str {
    "      VELNOR_ASSET_SHA256: 1111111111111111111111111111111111111111111111111111111111111111\n      VELNOR_ASSET_URL: https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu\n      VELNOR_RELEASE_COMMIT: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
}

#[test]
fn qualification_modes_accept_exact_files_without_environment() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let manifest = root.join("manifest.json");
    fs::write(&manifest, valid_manifest())?;
    let result = invoke(&[OsStr::new("qualification-manifest"), manifest.as_os_str()])?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, Vec::<u8>::new());
    let workflows = root.join(".github");
    fs::create_dir(&workflows)?;
    fs::write(workflows.join("ci.yml"), workflow())?;
    let result = invoke(&[
        OsStr::new("qualification-output-bindings"),
        manifest.as_os_str(),
        workflows.as_os_str(),
    ])?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, Vec::<u8>::new());
    fs::write(
        workflows.join("ci.yml"),
        workflow().replace(
            "1111111111111111111111111111111111111111111111111111111111111111",
            "4444444444444444444444444444444444444444444444444444444444444444",
        ),
    )?;
    let rejected = invoke(&[
        OsStr::new("qualification-output-bindings"),
        manifest.as_os_str(),
        workflows.as_os_str(),
    ])?;
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("archive guard: candidate output binding mismatch: wrong digest for target")
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn qualification_modes_retain_diagnostics_and_reject_wrong_arity() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let manifest = root.join("manifest.json");
    fs::write(&manifest, "{}")?;
    let invalid = invoke(&[OsStr::new("qualification-manifest"), manifest.as_os_str()])?;
    assert!(!invalid.status.success());
    assert!(
        String::from_utf8_lossy(&invalid.stderr)
            .contains("archive guard: candidate manifest validation:")
    );

    let missing_path = invoke(&[OsStr::new("qualification-manifest")])?;
    assert!(
        String::from_utf8_lossy(&missing_path.stderr)
            .contains("candidate manifest path is required")
    );
    let extra = invoke(&[
        OsStr::new("qualification-manifest"),
        manifest.as_os_str(),
        OsStr::new("extra"),
    ])?;
    assert!(String::from_utf8_lossy(&extra.stderr).contains("unexpected archive guard argument"));
    let missing_root = invoke(&[
        OsStr::new("qualification-output-bindings"),
        manifest.as_os_str(),
    ])?;
    assert!(
        String::from_utf8_lossy(&missing_root.stderr)
            .contains("generated workflow root is required")
    );
    let extra_output_arg = invoke(&[
        OsStr::new("qualification-output-bindings"),
        manifest.as_os_str(),
        OsStr::new("root"),
        OsStr::new("extra"),
    ])?;
    assert!(
        String::from_utf8_lossy(&extra_output_arg.stderr)
            .contains("unexpected archive guard argument")
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn qualification_paths_accept_unicode_and_spaces() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let manifest = root.join("manifest café copy.json");
    fs::write(&manifest, valid_manifest())?;
    let accepted = invoke(&[OsStr::new("qualification-manifest"), manifest.as_os_str()])?;
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn qualification_mode_must_be_utf8() -> Result<(), Box<dyn Error>> {
    let bad_mode = OsString::from_vec(b"qualification-\xff".to_vec());
    let rejected = invoke(&[bad_mode.as_os_str()])?;
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("archive guard mode is not UTF-8"));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn qualification_manifest_path_accepts_non_utf8_on_linux_filesystems() -> Result<(), Box<dyn Error>>
{
    let root = fixture_root()?;
    let manifest = root.join(OsString::from_vec(b"manifest-\xff.json".to_vec()));
    fs::write(&manifest, valid_manifest())?;
    let accepted = invoke(&[OsStr::new("qualification-manifest"), manifest.as_os_str()])?;
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    fs::remove_dir_all(root)?;
    Ok(())
}
