//! Public local release-manifest file-type verification.

use std::error::Error;
use std::fs;
use std::process::Command;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_orchestrator::local_release_manifest::verify_local_generator_release_manifest;

const SOURCE_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LINUX_BYTES: &[u8] = b"linux-binary\n";
const LINUX_SHA256: &str = "c8129af264901ca76e037945f033c1dc7b05f000e2eeec96d74ea0a69d9bfb61";
const MACOS_SHA256: &str = "6e851e6fefc18be0de3030587e30b1a1130f115473d34f264235735c6323b3eb";
const MACOS_X64_SHA256: &str = "b9ce8bf39067b6ba098879151d887bcadef7e1f4049131e0e703ba6ab37f6647";

#[cfg(unix)]
#[test]
fn fifo_binary_is_rejected_without_blocking() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let manifest = directory.path().join("manifest.json");
    let linux = directory.path().join("linux-generator");
    let macos_fifo = directory.path().join("macos-generator-fifo");
    fs::write(&linux, LINUX_BYTES)?;
    let status = Command::new("mkfifo").arg(&macos_fifo).status()?;
    assert!(
        status.success(),
        "mkfifo must create the macOS fixture FIFO"
    );
    fs::write(
        &manifest,
        serde_json::to_vec(&json!({
            "schema": 1,
            "version": "1.2.3",
            "repository": "tailrocks/velnor-new",
            "commit": SOURCE_COMMIT,
            "targets": [
                {
                    "target": "aarch64-apple-darwin",
                    "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v1.2.3/velnor-actions-1.2.3-aarch64-apple-darwin",
                    "sha256": MACOS_SHA256
                },
                {
                    "target": "x86_64-unknown-linux-gnu",
                    "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v1.2.3/velnor-actions-1.2.3-x86_64-unknown-linux-gnu",
                    "sha256": LINUX_SHA256
                },
                {
                    "target": "x86_64-apple-darwin",
                    "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v1.2.3/velnor-actions-1.2.3-x86_64-apple-darwin",
                    "sha256": MACOS_X64_SHA256
                }
            ]
        }))?,
    )?;
    let error =
        verify_local_generator_release_manifest(&manifest, SOURCE_COMMIT, &linux, &macos_fifo)
            .err()
            .ok_or("FIFO must be refused as a regular binary")?;
    assert!(
        error.to_string().contains("not_a_regular_file"),
        "unexpected FIFO error: {error}"
    );
    directory.close()?;
    Ok(())
}
