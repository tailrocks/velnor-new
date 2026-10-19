use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::download::{DownloadVerification, download_to_file_with_timeout_observed};
use super::{ProviderFailure, Workspace, download_to_file, download_to_file_with_timeout};

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
    fs::remove_file(pid_path).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn cancelling_download_removes_partial_file_and_stops_child() -> Result<(), String> {
    let pid_path = std::env::temp_dir().join(format!("velnor-gh-cancel-{}.pid", Uuid::new_v4()));
    let script = format!(
        "printf '%s\\n' \"$$\" > '{}'; printf 'abc'; exec /bin/sleep 30",
        shell_path(&pid_path)
    );
    let workspace = workspace(&script)?;
    let destination = workspace.assets().join("cancelled.tar");
    let digest = sha256(b"abcd");
    let mut transfer = Box::pin(download_to_file(
        &workspace,
        "test-secret",
        std::iter::empty(),
        &destination,
        4,
        4,
        &digest,
    ));
    tokio::select! {
        result = &mut transfer => return Err(format!("download ended before cancellation: {result:?}")),
        result = wait_for_partial_file(&destination, 3) => result?,
    }
    drop(transfer);

    let pid = fs::read_to_string(&pid_path).map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let alive = std::process::Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !destination.exists() && !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "cancellation did not clean the file and stop the child".to_owned())?;
    fs::remove_file(pid_path).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn cancelling_after_stdout_eof_removes_file_and_stops_child() -> Result<(), String> {
    let pid_path = std::env::temp_dir().join(format!("velnor-gh-eof-{}.pid", Uuid::new_v4()));
    let script = format!(
        "printf '%s\\n' \"$$\" > '{}'; printf 'abcd'; exec 1>&-; exec /bin/sleep 30",
        shell_path(&pid_path)
    );
    let workspace = workspace(&script)?;
    let destination = workspace.assets().join("eof-cancelled.tar");
    let (copy_complete_tx, copy_complete_rx) = tokio::sync::oneshot::channel();
    let mut transfer = Box::pin(download_to_file_with_timeout_observed(
        &workspace,
        "test-secret",
        std::iter::empty(),
        &destination,
        DownloadVerification {
            expected_size: 4,
            max_size: 4,
            expected_sha256: sha256(b"abcd"),
            timeout: Duration::from_secs(5),
        },
        copy_complete_tx,
    ));
    tokio::select! {
        result = &mut transfer => return Err(format!("download ended before cancellation: {result:?}")),
        result = copy_complete_rx => result.map_err(|error| error.to_string())?,
    }
    drop(transfer);

    let pid = fs::read_to_string(&pid_path).map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let alive = std::process::Command::new("/bin/kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !destination.exists() && !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "EOF cancellation did not clean the file and stop the child".to_owned())?;
    fs::remove_file(pid_path).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn download_timeout_kills_child_and_removes_partial_file() -> Result<(), String> {
    let pid_path = std::env::temp_dir().join(format!("velnor-gh-timeout-{}.pid", Uuid::new_v4()));
    let script = format!(
        "printf '%s\\n' \"$$\" > '{}'; printf 'abc'; exec /bin/sleep 30",
        shell_path(&pid_path)
    );
    let workspace = workspace(&script)?;
    let destination = workspace.assets().join("timed-out.tar");
    let digest = sha256(b"abcd");
    let mut transfer = Box::pin(download_to_file_with_timeout(
        &workspace,
        "test-secret",
        std::iter::empty(),
        &destination,
        DownloadVerification {
            expected_size: 4,
            max_size: 4,
            expected_sha256: digest,
            timeout: Duration::from_secs(5),
        },
    ));
    tokio::select! {
        result = &mut transfer => return Err(format!("download ended before timeout: {result:?}")),
        result = async {
            wait_for_file(&pid_path).await?;
            wait_for_partial_file(&destination, 3).await
        } => result?,
    }
    let result = tokio::time::timeout(Duration::from_secs(7), &mut transfer)
        .await
        .map_err(|_| "download timeout did not stop the child".to_owned())?;

    assert_eq!(result, Err(ProviderFailure::GhTimeout));
    assert!(!destination.exists());
    let pid = fs::read_to_string(&pid_path).map_err(|error| error.to_string())?;
    let alive = std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    assert!(!alive.success(), "timed-out child must be reaped");
    fs::remove_file(pid_path).map_err(|error| error.to_string())?;
    Ok(())
}

async fn wait_for_partial_file(path: &Path, expected_size: u64) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if fs::metadata(path).is_ok_and(|metadata| metadata.len() == expected_size) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "download did not write the partial file".to_owned())
}

async fn wait_for_file(path: &Path) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if path.is_file() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "child did not write its process marker".to_owned())
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
        cleanup_verified: false,
    })
}

#[test]
fn verified_workspace_cleanup_returns_only_after_exact_root_is_absent() -> Result<(), String> {
    let workspace = workspace("true")?;
    let root = workspace.root().to_path_buf();
    let receipt = workspace
        .remove_verified()
        .map_err(|error| format!("verified cleanup failed: {error:?}"))?;
    if root.exists() || receipt.workspace_digest().len() != 64 {
        return Err("cleanup receipt returned before workspace removal".to_owned());
    }
    Ok(())
}

#[test]
fn replaced_workspace_root_does_not_produce_cleanup_receipt() -> Result<(), String> {
    let workspace = workspace("true")?;
    let root = workspace.root().to_path_buf();
    fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    fs::write(&root, b"replacement").map_err(|error| error.to_string())?;
    let result = workspace.remove_verified();
    assert_eq!(result.err(), Some(ProviderFailure::WorkspaceCleanup));
    fs::remove_file(root).map_err(|error| error.to_string())?;
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    super::super::hex(&Sha256::digest(bytes))
}

fn shell_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}
