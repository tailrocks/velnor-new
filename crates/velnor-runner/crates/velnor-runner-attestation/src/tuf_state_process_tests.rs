use std::fs;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tempfile::TempDir;

use super::tests::{generation_path, root_fixture, seal_stage};
use super::*;

const PROCESS_MODE: &str = "VELNOR_CACHE_PROCESS_TEST_MODE";
const PROCESS_ROOT: &str = "VELNOR_CACHE_PROCESS_TEST_ROOT";
const PROCESS_MARKER: &str = "VELNOR_CACHE_PROCESS_TEST_MARKER";

fn process_test_child(mode: &str, root: &Path, marker: &Path) -> Child {
    Command::new(std::env::current_exe().expect("locate current test executable"))
        .args([
            "--exact",
            "tuf_state::process_tests::process_boundary_worker",
            "--nocapture",
        ])
        .env(PROCESS_MODE, mode)
        .env(PROCESS_ROOT, root)
        .env(PROCESS_MARKER, marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn isolated process test worker")
}

async fn wait_for_marker(child: &mut Child, marker: &Path) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if marker.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll test child") {
            panic!("test child exited before marker: {status}");
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "test child marker timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn kill_child(child: &mut Child) {
    child.kill().expect("send SIGKILL to test worker");
    let status = child.wait().expect("reap killed test worker");
    assert_eq!(status.signal(), Some(libc::SIGKILL));
}

#[tokio::test]
async fn process_boundary_worker() {
    let Ok(mode) = std::env::var(PROCESS_MODE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(PROCESS_ROOT).expect("worker cache root"));
    let marker = PathBuf::from(std::env::var_os(PROCESS_MARKER).expect("worker marker"));
    let cache = TufCache::new(&root);
    cache.prepare_root().expect("prepare worker cache root");
    let _lock = CacheLock::acquire(&root, Duration::from_secs(2))
        .await
        .expect("worker acquires process lock");

    if mode == "hold-lock" {
        fs::write(&marker, b"locked").expect("signal lock acquired");
        tokio::time::sleep(Duration::from_secs(60)).await;
        return;
    }

    let active = cache.read_active().expect("read worker active generation");
    let generation = cache
        .create_staging(active.as_ref())
        .expect("create worker staged generation");
    seal_stage(&generation, &root_fixture());
    let id = generation.id.clone();
    cache
        .place_generation(&generation)
        .expect("durably place worker generation");
    match mode.as_str() {
        "initial-after-place" | "existing-after-place" => {}
        "existing-before-pointer-rename" | "existing-after-pointer-rename" => {
            let pointer_temp = root.join(format!("CURRENT.tmp-{id}"));
            write_file(&pointer_temp, format!("{id}\n").as_bytes(), 0o600)
                .expect("write temporary active pointer");
            if mode == "existing-after-pointer-rename" {
                fs::rename(pointer_temp, root.join(CURRENT_FILE))
                    .expect("atomically rename active pointer");
            }
        }
        _ => panic!("unknown process boundary mode"),
    }
    fs::write(&marker, id.as_bytes()).expect("signal crash boundary ready");
    tokio::time::sleep(Duration::from_secs(60)).await;
}

#[tokio::test]
async fn cache_lock_excludes_a_distinct_process() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary root")
        .join("cache");
    let cache = TufCache::new(&root);
    cache.prepare_root().expect("prepare private cache root");
    let marker = temporary.path().join("worker-ready");
    let mut child = process_test_child("hold-lock", &root, &marker);
    wait_for_marker(&mut child, &marker).await;

    let blocked = CacheLock::acquire(&root, Duration::from_millis(150)).await;
    assert!(matches!(
        blocked,
        Err(ref error) if error.kind() == std::io::ErrorKind::TimedOut
    ));
    kill_child(&mut child);
    let released = CacheLock::acquire(&root, Duration::from_secs(1))
        .await
        .expect("SIGKILL releases the process-owned lock");
    drop(released);
}

#[tokio::test]
async fn sigkill_at_generation_and_pointer_boundaries_keeps_atomic_state() {
    for mode in [
        "initial-after-place",
        "existing-after-place",
        "existing-before-pointer-rename",
        "existing-after-pointer-rename",
    ] {
        let temporary = TempDir::new().expect("create isolated boundary cache");
        let root = fs::canonicalize(temporary.path())
            .expect("canonicalize temporary root")
            .join("cache");
        let cache = TufCache::new(&root);
        cache.prepare_root().expect("prepare private cache root");
        let old_id = if mode.starts_with("existing-") {
            let first = cache
                .create_staging(None)
                .expect("stage initial generation");
            seal_stage(&first, &root_fixture());
            cache.publish(&first).expect("publish initial generation");
            Some(first.id.clone())
        } else {
            None
        };
        let marker = temporary.path().join("worker-ready");
        let mut child = process_test_child(mode, &root, &marker);
        wait_for_marker(&mut child, &marker).await;
        let unactivated_id = fs::read_to_string(&marker).expect("read worker generation id");
        kill_child(&mut child);

        let _lock = CacheLock::acquire(&root, Duration::from_secs(1))
            .await
            .expect("SIGKILL releases cache lock");
        match mode {
            "initial-after-place" => {
                assert!(
                    cache.read_active().is_err(),
                    "orphan generation is fail-closed"
                );
                assert!(!root.join(CURRENT_FILE).exists());
            }
            "existing-after-place" | "existing-before-pointer-rename" => {
                assert_eq!(
                    cache
                        .read_active()
                        .expect("read old active generation")
                        .expect("active generation is present")
                        .id,
                    old_id.expect("active generation is present")
                );
                assert!(generation_path(&root, unactivated_id.trim()).is_dir());
            }
            "existing-after-pointer-rename" => {
                assert_eq!(
                    cache
                        .read_active()
                        .expect("read new active generation")
                        .expect("active generation is present")
                        .id,
                    unactivated_id.trim()
                );
                assert_eq!(
                    fs::read_to_string(root.join(CURRENT_FILE)).expect("read active pointer"),
                    format!("{}\n", unactivated_id.trim())
                );
            }
            _ => unreachable!(),
        }
    }
}

fn tree_file_bytes(path: &Path) -> u64 {
    let mut total = 0_u64;
    for entry in fs::read_dir(path).expect("read generation directory") {
        let path = entry.expect("read generation entry").path();
        let metadata = fs::symlink_metadata(&path).expect("stat generation entry");
        if metadata.is_dir() {
            total += tree_file_bytes(&path);
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }
    total
}

#[test]
fn cache_byte_limit_is_per_generation_not_a_global_disk_cap() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary root")
        .join("cache");
    let cache = TufCache::new(&root);
    cache.prepare_root().expect("prepare private cache root");
    let payload = vec![0_u8; 13 * 1024 * 1024];
    let first = cache.create_staging(None).expect("stage first generation");
    let first_id = first.id.clone();
    for index in 0..3 {
        write_file(
            &first.path.join(format!("large-{index}.bin")),
            &payload,
            0o600,
        )
        .expect("write bounded fixture payload");
    }
    seal_stage(&first, &root_fixture());
    cache.publish(&first).expect("publish first generation");

    let active = cache
        .read_active()
        .expect("read active generation")
        .expect("active generation is present");
    let second = cache
        .create_staging(Some(&active))
        .expect("stage second generation");
    for index in 0..3 {
        write_file(
            &second.path.join(format!("large-{index}.bin")),
            &payload,
            0o600,
        )
        .expect("write bounded fixture payload");
    }
    seal_stage(&second, &root_fixture());
    cache.publish(&second).expect("publish second generation");

    let active = cache
        .read_active()
        .expect("read second active")
        .expect("active generation is present");
    let active_bytes = tree_file_bytes(&active.path);
    let old_path = generation_path(&root, &first_id);
    let old_bytes = tree_file_bytes(&old_path);
    assert!(active_bytes < MAX_CACHE_BYTES);
    assert!(old_bytes < MAX_CACHE_BYTES);
    assert!(active_bytes + old_bytes > MAX_CACHE_BYTES);
    assert!(hash_tree_files(&active.path).is_ok());
    assert!(hash_tree_files(&old_path).is_ok());
}
