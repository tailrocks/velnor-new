use std::error::Error;
use std::path::Path;

use super::archive_guard_inputs;
use super::{gzip, invoke, producer_tar};

#[test]
fn accepts_release_producer_raw_tar_from_stdin_without_environment() -> Result<(), Box<dyn Error>> {
    let input = producer_tar()?;
    let output = invoke(&["candidate"], &input)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn rejects_gzip_candidate_and_raw_cargo_package_without_format_sniffing()
-> Result<(), Box<dyn Error>> {
    let raw = producer_tar()?;
    let compressed = gzip(&raw)?;

    let candidate = invoke(&["candidate"], &compressed)?;
    assert!(!candidate.status.success());
    assert!(!String::from_utf8_lossy(&candidate.stderr).is_empty());

    let cargo_package = invoke(&["cargo-package"], &raw)?;
    assert!(!cargo_package.status.success());
    assert!(!String::from_utf8_lossy(&cargo_package.stderr).is_empty());
    Ok(())
}

#[test]
fn accepts_cargo_package_gzip_tar_from_stdin_without_environment() -> Result<(), Box<dyn Error>> {
    let input = gzip(&producer_tar()?)?;
    let output = invoke(&["cargo-package"], &input)?;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn rejects_extra_arguments_and_retired_modes() -> Result<(), Box<dyn Error>> {
    let extra = invoke(&["candidate", "unexpected"], &[])?;
    assert!(!extra.status.success());
    assert!(String::from_utf8_lossy(&extra.stderr).contains("unexpected archive guard argument"));

    let retired = invoke(&["source"], &[])?;
    assert!(!retired.status.success());
    assert!(
        String::from_utf8_lossy(&retired.stderr).contains("unknown archive guard mode: source")
    );
    Ok(())
}

#[test]
fn reports_a_lowercase_source_fingerprint() -> Result<(), Box<dyn Error>> {
    let output = invoke(&["--fingerprint"], &[])?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let fingerprint = String::from_utf8(output.stdout)?;
    assert_eq!(fingerprint.len(), 65);
    assert!(fingerprint.ends_with('\n'));
    assert!(
        fingerprint[..64]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("workspace root is missing")?;
    let (expected, _) = archive_guard_inputs::fingerprint(workspace)?;
    assert_eq!(fingerprint, format!("{expected}\n"));
    Ok(())
}
