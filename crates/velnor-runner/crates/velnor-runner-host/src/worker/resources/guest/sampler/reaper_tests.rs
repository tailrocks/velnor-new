use std::error::Error;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use super::super::super::GuestSampleFailure;
use super::{SamplerJoinReaper, SamplerJoinRequest};
use tokio::sync::watch;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn failed_reaper_start_retains_and_later_joins_sampler() -> Result<(), Box<dyn Error>> {
    let reaper = SamplerJoinReaper::new();
    let (release, waiting) = mpsc::channel();
    let sampler_finished = Arc::new(AtomicBool::new(false));
    let thread_finished = Arc::clone(&sampler_finished);
    let sampler = thread::spawn(move || {
        waiting
            .recv()
            .map_err(|_| GuestSampleFailure::SamplerTask)?;
        thread_finished.store(true, Ordering::SeqCst);
        Err(GuestSampleFailure::SamplerTask)
    });
    let (failure_sender, mut failures) = watch::channel(None);
    let report_failure = failure_sender.clone();
    let request = SamplerJoinRequest {
        thread: sampler,
        task_failure: failure_sender,
    };

    let startup = reaper.enqueue_with(request, |_| Err(GuestSampleFailure::RuntimeUnavailable));
    assert_eq!(startup, Err(GuestSampleFailure::RuntimeUnavailable));
    let queued = match reaper.queue.state.lock() {
        Ok(state) => state.requests.len(),
        Err(poisoned) => poisoned.into_inner().requests.len(),
    };
    assert_eq!(queued, 1);
    let _ = report_failure.send_replace(Some(GuestSampleFailure::RuntimeUnavailable));
    release
        .send(())
        .map_err(|_| std::io::Error::other("sampler release receiver closed"))?;
    reaper
        .ensure_worker()
        .map_err(|_| std::io::Error::other("join reaper retry failed"))?;

    timeout(Duration::from_secs(2), async {
        loop {
            if *failures.borrow() == Some(GuestSampleFailure::SamplerTask) {
                break;
            }
            failures.changed().await?;
            sleep(Duration::from_millis(2)).await;
        }
        Ok::<(), tokio::sync::watch::error::RecvError>(())
    })
    .await??;
    assert!(sampler_finished.load(Ordering::SeqCst));
    reaper
        .shutdown()
        .map_err(|_| std::io::Error::other("join reaper shutdown failed"))?;
    Ok(())
}

#[test]
fn stopping_idle_reaper_cannot_miss_condvar_notification() -> Result<(), Box<dyn Error>> {
    const CHILD_MARKER: &str = "VELNOR_REAPER_STOP_BOUNDARY_CHILD";
    if std::env::var_os(CHILD_MARKER).is_some() {
        return reaper_stop_boundary_child();
    }

    let test_binary = std::env::current_exe()?;
    let mut child = Command::new(test_binary)
        .args([
            "--exact",
            "worker::resources::guest::sampler::cache::reaper::tests::stopping_idle_reaper_cannot_miss_condvar_notification",
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
                "reaper boundary child exited with {}",
                output.status
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("stopping_idle_reaper_cannot_miss_condvar_notification ... ok"),
                "child did not run the selected test: {stdout}"
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _output = child.wait_with_output()?;
            return Err(std::io::Error::other("reaper boundary child exceeded watchdog").into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn reaper_stop_boundary_child() -> Result<(), Box<dyn Error>> {
    let reaper = SamplerJoinReaper::new();
    let reached = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    reaper.set_wait_hook(Arc::clone(&reached), Arc::clone(&resume));
    reaper
        .ensure_worker()
        .map_err(|_| std::io::Error::other("join reaper did not start"))?;
    reached.wait();

    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        resume.wait();
    });
    reaper
        .shutdown()
        .map_err(|_| std::io::Error::other("idle reaper did not stop"))?;
    release
        .join()
        .map_err(|_| std::io::Error::other("boundary release thread panicked"))?;
    Ok(())
}
