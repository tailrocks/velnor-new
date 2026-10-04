//! CLI dispatch coverage for private generator release-manifest operations.

use std::error::Error;
use std::process::Command;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

const ASSET_DIR_ENV: &str = "VELNOR_RELEASE_ASSET_DIR";
const VERSION_ENV: &str = "VELNOR_RELEASE_VERSION";
const TAG_ENV: &str = "VELNOR_RELEASE_TAG";
const REPOSITORY_ENV: &str = "VELNOR_RELEASE_REPOSITORY";
const SOURCE_ENV: &str = "VELNOR_SOURCE_SHA";
const ASSEMBLE_OP: &str = "assemble-generator-release-manifest-v1";
const VERIFY_OP: &str = "verify-generator-release-manifest-v1";

const TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

#[test]
fn release_manifest_operations_fail_closed_without_context() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("generator-manifest-gate")?;
    for operation in [
        "assemble-generator-release-manifest-v1",
        "verify-generator-release-manifest-v1",
    ] {
        let output = spawn_isolated(&[], &[("VELNOR_INTERNAL_OP", operation)], &tmp)?;
        assert_eq!(code(&output), 1, "{operation}");
        assert!(output.stdout.is_empty(), "internal stdout must stay empty");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(stderr.contains("internal request failed"), "{stderr}");
        assert!(!stderr.contains(operation), "{stderr}");
        assert!(!stderr.contains("VELNOR_INTERNAL"), "{stderr}");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn release_manifest_operations_assemble_and_verify_explicit_context() -> Result<(), Box<dyn Error>>
{
    let tmp = fresh_tempdir("generator-manifest-run")?;
    let assets = tmp.join("assets");
    std::fs::create_dir(&assets)?;
    let version = env!("CARGO_PKG_VERSION");
    let tag = format!("v{version}");
    let source = "ab".repeat(20);
    for target in TARGETS {
        let name = format!("velnor-actions-{version}-{target}");
        let bytes = format!("fixture bytes: {target}\n");
        std::fs::write(assets.join(&name), &bytes)?;
        let output = Command::new("shasum")
            .args(["-a", "256", &name])
            .current_dir(&assets)
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8(output.stderr)?.into());
        }
        let line = String::from_utf8(output.stdout)?;
        let digest = line.split_whitespace().next().ok_or("missing SHA-256")?;
        std::fs::write(
            assets.join(format!("{name}.sha256")),
            format!("{digest}  {name}\n"),
        )?;
    }
    let asset_dir = assets.to_str().ok_or("asset path is not UTF-8")?;
    let context = [
        (ASSET_DIR_ENV, asset_dir),
        (VERSION_ENV, version),
        (TAG_ENV, tag.as_str()),
        (REPOSITORY_ENV, "tailrocks/velnor-new"),
        (SOURCE_ENV, source.as_str()),
    ];
    let mut assemble = context.to_vec();
    assemble.push(("VELNOR_INTERNAL_OP", ASSEMBLE_OP));
    let result = spawn_isolated(&[], &assemble, &tmp)?;
    assert_eq!(
        code(&result),
        0,
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());

    let manifest = std::fs::read(assets.join("velnor-actions-release-manifest.json"))?;
    let value: serde_json::Value = serde_json::from_slice(&manifest)?;
    assert_eq!(value["version"], version);
    assert_eq!(value["repository"], "tailrocks/velnor-new");
    assert_eq!(value["commit"], source);
    assert_eq!(
        value["targets"].as_array().ok_or("targets missing")?.len(),
        3
    );

    let mut verify = context.to_vec();
    verify.push(("VELNOR_INTERNAL_OP", VERIFY_OP));
    let result = spawn_isolated(&[], &verify, &tmp)?;
    assert_eq!(
        code(&result),
        0,
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    cleanup(&tmp);
    Ok(())
}
