use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{ProviderFailure, Workspace, download_to_file};

#[tokio::test]
async fn bounded_download_writes_only_exact_size_and_digest() -> Result<(), String> {
    let workspace = workspace("printf 'verified-asset'")?;
    let destination = workspace.assets().join("asset.tar");
    let bytes = b"verified-asset";
    let result = download_to_file(
        &workspace,
        "test-secret",
        std::iter::empty(),
        &destination,
        bytes.len() as u64,
        64,
        &sha256(bytes),
    )
    .await;

    assert_eq!(result, Ok(()));
    assert_eq!(
        fs::read(destination).map_err(|error| error.to_string())?,
        bytes
    );
    Ok(())
}

#[tokio::test]
async fn oversized_download_kills_and_waits_for_child_and_removes_partial_file()
-> Result<(), String> {
    let pid_path = std::env::temp_dir().join(format!("velnor-gh-child-{}.pid", Uuid::new_v4()));
    let script = format!(
        "printf '%s\\n' \"$$\" > '{}'; printf 'oversized-asset'; exec /bin/sleep 30",
        shell_path(&pid_path)
    );
    let workspace = workspace(&script)?;
    let destination = workspace.assets().join("asset.tar");
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        download_to_file(
            &workspace,
            "test-secret",
            std::iter::empty(),
            &destination,
            4,
            4,
            &sha256(b"data"),
        ),
    )
    .await
    .map_err(|_| "overflow handling did not stop the child promptly".to_owned())?;

    assert_eq!(result, Err(ProviderFailure::ReleaseAssets));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!destination.exists());
    let pid = fs::read_to_string(&pid_path).map_err(|error| error.to_string())?;
    let alive = std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .status()
        .map_err(|error| error.to_string())?;
    assert!(!alive.success(), "over-limit child must be reaped");
    let _ = fs::remove_file(pid_path);
    Ok(())
}

#[tokio::test]
async fn short_or_wrong_digest_download_is_removed() -> Result<(), String> {
    for (name, script, expected_size, expected_digest) in [
        ("short", "printf 'abc'", 4, sha256(b"abcd")),
        ("digest", "printf 'abcd'", 4, "0".repeat(64)),
    ] {
        let workspace = workspace(script)?;
        let destination = workspace.assets().join(format!("{name}.tar"));
        let result = download_to_file(
            &workspace,
            "test-secret",
            std::iter::empty(),
            &destination,
            expected_size,
            8,
            &expected_digest,
        )
        .await;
        assert_eq!(result, Err(ProviderFailure::ReleaseAssets));
        assert!(!destination.exists());
    }
    Ok(())
}

fn workspace(script: &str) -> Result<Workspace, String> {
    let root = std::env::temp_dir().join(format!("velnor-gh-test-{}", Uuid::new_v4()));
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    let assets = root.join("assets");
    let config = root.join("gh-config");
    fs::create_dir(&assets).map_err(|error| error.to_string())?;
    fs::create_dir(&config).map_err(|error| error.to_string())?;
    let executable = root.join("fake-gh");
    fs::write(&executable, format!("#!/bin/sh\n{script}\n")).map_err(|error| error.to_string())?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    Ok(Workspace {
        executable,
        root,
        assets,
        config,
    })
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn shell_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}
