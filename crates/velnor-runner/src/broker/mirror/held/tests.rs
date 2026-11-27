// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, TryLockError};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::Held;

fn is_locked(path: &std::path::Path) -> bool {
    let file = File::open(path).expect("open the lock file");
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

#[tokio::test]
async fn a_cleanup_that_outlives_its_future_keeps_the_lock_until_it_returns() {
    let path = std::env::temp_dir().join(format!("velnor-held-{}", std::process::id()));
    let held = Held::acquire(&path).await.expect("acquire");
    let (started, running) = mpsc::channel();
    let (finish, finished) = mpsc::channel::<()>();
    let cleanup = tokio::spawn({
        let held = held.clone();
        async move {
            held.blocking(move || {
                started.send(()).expect("announce");
                let _ = finished.recv();
            })
            .await
        }
    });
    tokio::task::spawn_blocking(move || running.recv())
        .await
        .expect("join")
        .expect("the cleanup started");
    cleanup.abort();
    let _ = cleanup.await;
    drop(held);
    assert!(is_locked(&path), "the lock was released under the cleanup");
    finish.send(()).expect("let the cleanup return");
    let deadline = Instant::now() + Duration::from_secs(10);
    while is_locked(&path) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let released = !is_locked(&path);
    let _ = std::fs::remove_file(&path);
    assert!(released, "the lock was never released");
}
