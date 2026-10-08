use std::time::Duration;

use tokio::time::Instant;

use super::{acquire_slot, decode_slots, decode_with_deadline, receive_decode_result};
use crate::HostError;

#[tokio::test]
async fn ready_decode_result_after_cutoff_is_rejected() {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    sender
        .send(Ok::<_, HostError>(()))
        .expect("receiver exists");
    assert_eq!(
        receive_decode_result(receiver, Instant::now() - Duration::from_millis(1)).await,
        Err(HostError::Docker)
    );
}

#[test]
fn blocking_current_thread_runtime_drop_does_not_wait_for_held_decode() {
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let blocking_runtime = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime builds");
        let result = runtime.block_on(async move {
            let permit =
                acquire_slot(decode_slots(), Instant::now() + Duration::from_secs(1)).await?;
            decode_with_deadline(
                Vec::new(),
                Instant::now() + Duration::from_millis(80),
                permit,
                move |_| {
                    started_tx.send(()).map_err(|_| HostError::Docker)?;
                    release_rx.recv().map_err(|_| HostError::Docker)?;
                    Ok::<_, HostError>(())
                },
            )
            .await
        });
        drop(runtime);
        finished_tx
            .send(result)
            .expect("finished receiver remains live");
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("detached decoder starts");
    let finished = finished_rx.recv_timeout(Duration::from_millis(500));
    let runtime_returned_before_decode = finished.is_ok();
    release_tx
        .send(())
        .expect("decoder still waits for release");
    if let Ok(result) = finished {
        assert_eq!(result, Err(HostError::Docker));
    } else {
        assert_eq!(
            finished_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("runtime returns after releasing decoder"),
            Err(HostError::Docker)
        );
    }
    blocking_runtime
        .join()
        .expect("blocking runtime thread exits");
    assert!(runtime_returned_before_decode);
}
