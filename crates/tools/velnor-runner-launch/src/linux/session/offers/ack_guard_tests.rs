use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    BatchOutcome, BatchPipeline, KindOutcome, acknowledge_after_processing, run_batch_pipeline,
};

#[derive(Clone, Copy)]
enum ScriptedProcess {
    CapacityFull,
    AcquireUncertain,
    Complete,
}

struct ScriptedBatchPipeline {
    message_id: i64,
    process: ScriptedProcess,
    acknowledge_result: bool,
    delete_count: usize,
    events: Vec<&'static str>,
}

impl ScriptedBatchPipeline {
    fn new(process: ScriptedProcess) -> Self {
        Self {
            message_id: 41,
            process,
            acknowledge_result: true,
            delete_count: 0,
            events: Vec::new(),
        }
    }
}

impl BatchPipeline for ScriptedBatchPipeline {
    fn message_id(&self) -> i64 {
        self.message_id
    }

    fn has_available_offers(&self) -> bool {
        true
    }

    async fn persist_and_clean_lifecycle(&mut self) -> bool {
        self.events.push("PersistPollObservation");
        self.events.push("PersistStartedCompletedEvents");
        self.events.push("CleanupCompletedGeneration");
        std::future::ready(()).await;
        true
    }

    async fn process_offers(&mut self) -> KindOutcome {
        self.events.push("ReserveAllAvailable");
        std::future::ready(()).await;
        match self.process {
            ScriptedProcess::CapacityFull => {
                self.events.push("CapacityFull");
                KindOutcome::AvailableOffersHeld
            }
            ScriptedProcess::AcquireUncertain => {
                self.events.push("AcquireDispatchedUncertain");
                KindOutcome::AvailableOffersHeld
            }
            ScriptedProcess::Complete => {
                self.events.push("AcquireResolved");
                self.events.push("JitIssued");
                self.events.push("RunnerStarted");
                self.events.push("DurableLaunchDone");
                KindOutcome::Ready
            }
        }
    }

    async fn acknowledge_message(&mut self, message_id: i64) -> bool {
        assert_eq!(message_id, self.message_id);
        self.events.push("DeleteMessage");
        self.delete_count += 1;
        std::future::ready(()).await;
        self.acknowledge_result
    }
}

async fn run_scripted_linux_batch(
    pipeline: &mut ScriptedBatchPipeline,
    cutoff_pending: bool,
) -> (BatchOutcome, Option<crate::linux::LinuxShutdownGap>) {
    let processed = run_batch_pipeline(pipeline).await;
    let delivered = crate::linux::session::poll_lifecycle::finish_delivered_batch(
        processed,
        cutoff_pending,
        || async {
            pipeline.events.push("CloseSession");
            true
        },
    )
    .await;
    if let Some(stop) = crate::linux::session::session_stop_after_batch(delivered) {
        (delivered, crate::linux::shutdown_gap_after_session(stop))
    } else {
        pipeline.events.push("PollAgain");
        (delivered, None)
    }
}

#[tokio::test]
async fn held_available_batch_never_dispatches_whole_message_delete() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let outcome =
        acknowledge_after_processing(true, KindOutcome::AvailableOffersHeld, || async move {
            observed.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await;

    assert_eq!(outcome, BatchOutcome::AvailableOffersHeld);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ready_available_batch_deletes_once_but_uncertain_delete_holds_progress() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let outcome = acknowledge_after_processing(true, KindOutcome::Ready, || async move {
        observed.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(outcome, BatchOutcome::Advanced);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let outcome = acknowledge_after_processing(true, KindOutcome::Ready, || async { false }).await;
    assert_eq!(outcome, BatchOutcome::AvailableOffersHeld);
}

#[tokio::test]
async fn stopped_or_held_batch_never_invokes_acknowledgement() {
    let calls = Arc::new(AtomicUsize::new(0));
    for processed in [KindOutcome::Stopped, KindOutcome::AvailableOffersHeld] {
        let observed = Arc::clone(&calls);
        let outcome = acknowledge_after_processing(true, processed, || async move {
            observed.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await;
        assert_ne!(outcome, BatchOutcome::Advanced);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn completed_cleanup_precedes_overflow_and_held_message_stops_daemon_progress() {
    let mut pipeline = ScriptedBatchPipeline::new(ScriptedProcess::CapacityFull);
    let (outcome, gap) = run_scripted_linux_batch(&mut pipeline, true).await;

    assert_eq!(outcome, BatchOutcome::AvailableOffersHeld);
    assert_eq!(
        gap,
        Some(crate::linux::LinuxShutdownGap::AvailableOffersHeld)
    );
    assert_eq!(pipeline.delete_count, 0);
    assert_eq!(
        pipeline.events,
        [
            "PersistPollObservation",
            "PersistStartedCompletedEvents",
            "CleanupCompletedGeneration",
            "ReserveAllAvailable",
            "CapacityFull",
        ]
    );
}

#[tokio::test]
async fn uncertain_acquire_holds_message_without_delete_close_or_next_poll() {
    let mut pipeline = ScriptedBatchPipeline::new(ScriptedProcess::AcquireUncertain);
    let (outcome, gap) = run_scripted_linux_batch(&mut pipeline, true).await;

    assert_eq!(outcome, BatchOutcome::AvailableOffersHeld);
    assert_eq!(
        gap,
        Some(crate::linux::LinuxShutdownGap::AvailableOffersHeld)
    );
    assert_eq!(pipeline.delete_count, 0);
    assert_eq!(
        pipeline.events,
        [
            "PersistPollObservation",
            "PersistStartedCompletedEvents",
            "CleanupCompletedGeneration",
            "ReserveAllAvailable",
            "AcquireDispatchedUncertain",
        ]
    );
}

#[tokio::test]
async fn capacity_fit_runs_effects_then_one_delete_and_allows_next_poll() {
    let mut pipeline = ScriptedBatchPipeline::new(ScriptedProcess::Complete);
    let (outcome, gap) = run_scripted_linux_batch(&mut pipeline, false).await;

    assert_eq!(outcome, BatchOutcome::Advanced);
    assert_eq!(gap, None);
    assert_eq!(pipeline.delete_count, 1);
    assert_eq!(
        pipeline.events,
        [
            "PersistPollObservation",
            "PersistStartedCompletedEvents",
            "CleanupCompletedGeneration",
            "ReserveAllAvailable",
            "AcquireResolved",
            "JitIssued",
            "RunnerStarted",
            "DurableLaunchDone",
            "DeleteMessage",
            "PollAgain",
        ]
    );
}
