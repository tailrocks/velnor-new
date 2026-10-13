use std::error::Error;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::super::GuestSampleFailure;
use super::reaper::{SamplerThreadJoinGuard, ensure_sampler_join_reaper};
use tokio::sync::watch;
use tokio::time::timeout;

#[test]
fn aborted_queued_join_is_handed_to_owned_reaper() -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()?;
    let (sampler_release, sampler_wait) = mpsc::channel();
    let sampler_thread = thread::spawn(move || {
        sampler_wait
            .recv()
            .map_err(|_| GuestSampleFailure::SamplerTask)?;
        Err(GuestSampleFailure::SamplerTask)
    });
    let (failure_sender, mut failures) = watch::channel(None);
    let guard = SamplerThreadJoinGuard::new(sampler_thread, failure_sender);

    runtime.block_on(async {
        let (blocking_started, blocking_started_receiver) = tokio::sync::oneshot::channel();
        let (blocking_release, blocking_wait) = mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            blocking_started
                .send(())
                .map_err(|_| std::io::Error::other("blocking start signal closed"))?;
            blocking_wait
                .recv()
                .map_err(|_| std::io::Error::other("blocking release signal closed"))?;
            Ok::<(), std::io::Error>(())
        });
        timeout(Duration::from_secs(2), blocking_started_receiver).await??;

        let queued = tokio::task::spawn_blocking(move || drop(guard));
        tokio::task::yield_now().await;
        queued.abort();
        sampler_release
            .send(())
            .map_err(|_| std::io::Error::other("sampler release receiver closed"))?;
        blocking_release
            .send(())
            .map_err(|_| std::io::Error::other("blocking release receiver closed"))?;

        let queued_result = timeout(Duration::from_secs(2), queued).await?;
        assert!(queued_result.is_err_and(|error| error.is_cancelled()));
        blocker.await??;
        timeout(Duration::from_secs(2), async {
            loop {
                if *failures.borrow() == Some(GuestSampleFailure::SamplerTask) {
                    break;
                }
                failures.changed().await?;
            }
            Ok::<(), tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        Ok::<(), Box<dyn Error>>(())
    })?;
    Ok(())
}

#[test]
fn closed_retained_handle_drop_uses_reaper_without_runtime_reentry() -> Result<(), Box<dyn Error>> {
    const CHILD_MARKER: &str = "VELNOR_CLOSED_HANDLE_JOIN_CHILD";
    if std::env::var_os(CHILD_MARKER).is_some() {
        return closed_handle_child();
    }

    let test_binary = std::env::current_exe()?;
    let mut child = Command::new(test_binary)
        .args([
            "--exact",
            "worker::resources::guest::sampler::cache::cache_tests::closed_retained_handle_drop_uses_reaper_without_runtime_reentry",
            "--nocapture",
        ])
        .env(CHILD_MARKER, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            assert!(
                output.status.success(),
                "closed-handle child exited with {}",
                output.status
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains(
                    "closed_retained_handle_drop_uses_reaper_without_runtime_reentry ... ok"
                ),
                "child did not run the selected test: {stdout}"
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _output = child.wait_with_output()?;
            return Err(std::io::Error::other("closed-handle child exceeded watchdog").into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn closed_handle_child() -> Result<(), Box<dyn Error>> {
    ensure_sampler_join_reaper().map_err(|_| std::io::Error::other("join reaper did not start"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()?;
    let retained_handle = runtime.handle().clone();
    runtime.shutdown_background();

    let (release, waiting) = mpsc::channel();
    let sampler_thread = thread::spawn(move || {
        waiting
            .recv()
            .map_err(|_| GuestSampleFailure::SamplerTask)?;
        Err(GuestSampleFailure::SamplerTask)
    });
    let (failure_sender, failures) = watch::channel(None);
    let guard = SamplerThreadJoinGuard::new(sampler_thread, failure_sender);
    let (dropped, dropped_receiver) = mpsc::channel();
    let dropper = thread::spawn(move || {
        let _entered = retained_handle.enter();
        drop(guard);
        dropped
            .send(())
            .map_err(|_| std::io::Error::other("drop completion receiver closed"))
    });
    dropped_receiver.recv_timeout(Duration::from_secs(1))?;
    dropper
        .join()
        .map_err(|_| std::io::Error::other("closed-handle drop thread panicked"))??;
    release
        .send(())
        .map_err(|_| std::io::Error::other("sampler release receiver closed"))?;

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if *failures.borrow() == Some(GuestSampleFailure::SamplerTask) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err(std::io::Error::other("reaper did not join closed-handle task").into())
}
