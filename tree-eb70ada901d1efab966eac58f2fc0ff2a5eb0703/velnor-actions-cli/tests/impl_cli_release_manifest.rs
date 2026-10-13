//! Public CLI coverage for local release-manifest verification.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::impl_cli_tmp::{code, spawn};

const SOURCE_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LINUX_SHA256: &str = "c8129af264901ca76e037945f033c1dc7b05f000e2eeec96d74ea0a69d9bfb61";
const MACOS_SHA256: &str = "6e851e6fefc18be0de3030587e30b1a1130f115473d34f264235735c6323b3eb";

struct TempRoot {
    path: PathBuf,
    closed: bool,
}

impl TempRoot {
    fn new(prefix: &str) -> Result<Self, Box<dyn Error>> {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-cli-{prefix}-{}-{counter}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self {
            path,
            closed: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn close(mut self) -> Result<(), Box<dyn Error>> {
        fs::remove_dir_all(&self.path)?;
        self.closed = true;
        Ok(())
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if !self.closed
            && let Err(error) = fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("test tempdir cleanup failed: {error}");
        }
    }
}

#[test]
fn verify_release_manifest_accepts_local_target_binaries() -> Result<(), Box<dyn Error>> {
    let root = TempRoot::new("release-manifest-valid")?;
    let manifest = root.path().join("release.json");
    let linux = root.path().join("linux");
    let macos = root.path().join("macos");
    fs::write(&linux, b"linux-binary\n")?;
    fs::write(&macos, b"macos-binary\n")?;
    fs::write(&manifest, serde_json::to_vec(&valid_manifest())?)?;
    let output = invoke(&manifest, &linux, &macos, SOURCE_COMMIT, root.path())?;
    assert_eq!(code(&output), 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("match the release manifest"));
    root.close()?;
    Ok(())
}

#[test]
fn verify_release_manifest_reports_a_binary_digest_mismatch() -> Result<(), Box<dyn Error>> {
    let root = TempRoot::new("release-manifest-mismatch")?;
    let manifest = root.path().join("release.json");
    let linux = root.path().join("linux");
    let macos = root.path().join("macos");
    fs::write(&linux, b"changed\n")?;
    fs::write(&macos, b"macos-binary\n")?;
    fs::write(&manifest, serde_json::to_vec(&valid_manifest())?)?;
    let output = invoke(&manifest, &linux, &macos, SOURCE_COMMIT, root.path())?;
    assert_eq!(code(&output), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest_mismatch"));
    root.close()?;
    Ok(())
}

fn invoke(
    manifest: &std::path::Path,
    linux: &std::path::Path,
    macos: &std::path::Path,
    commit: &str,
    cwd: &std::path::Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    let args = [
        "verify-release-manifest",
        "--manifest",
        manifest.to_str().ok_or("manifest path is UTF-8")?,
        "--expected-source-commit",
        commit,
        "--linux-x64-binary",
        linux.to_str().ok_or("Linux path is UTF-8")?,
        "--macos-arm64-binary",
        macos.to_str().ok_or("macOS path is UTF-8")?,
    ];
    spawn(&args, &[], cwd)
}

fn valid_manifest() -> serde_json::Value {
    json!({
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
            }
        ]
    })
}
