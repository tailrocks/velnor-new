#[cfg(test)]
#[path = "ack_guard_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::linux::session) enum BatchOutcome {
    Advanced,
    Stopped,
    AvailableOffersHeld,
}

pub(super) enum KindOutcome {
    Ready,
    Stopped,
    AvailableOffersHeld,
}

/// Ordered message stages used by both the production adapter and scripted
/// orchestration checks. Production owns the Journal, worker, and protocol
/// effects behind these stages; tests only replace those external services.
pub(super) trait BatchPipeline {
    fn message_id(&self) -> i64;

    fn has_available_offers(&self) -> bool;

    async fn persist_and_clean_lifecycle(&mut self) -> bool;

    async fn process_offers(&mut self) -> KindOutcome;

    async fn acknowledge_message(&mut self, message_id: i64) -> bool;
}

/// Run the production message ordering: durable lifecycle cleanup, offer
/// effects, then one whole-message acknowledgement when every stage is ready.
pub(super) async fn run_batch_pipeline<P: BatchPipeline>(pipeline: &mut P) -> BatchOutcome {
    if !pipeline.persist_and_clean_lifecycle().await {
        return BatchOutcome::Stopped;
    }
    let available = pipeline.has_available_offers();
    let message_id = pipeline.message_id();
    let processed = pipeline.process_offers().await;
    acknowledge_after_processing(available, processed, || {
        pipeline.acknowledge_message(message_id)
    })
    .await
}

pub(super) async fn acknowledge_after_processing<F, Fut>(
    available_batch: bool,
    processed: KindOutcome,
    acknowledge: F,
) -> BatchOutcome
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    match processed {
        KindOutcome::Stopped => return BatchOutcome::Stopped,
        KindOutcome::AvailableOffersHeld => return BatchOutcome::AvailableOffersHeld,
        KindOutcome::Ready => {}
    }
    if acknowledge().await {
        BatchOutcome::Advanced
    } else if available_batch {
        // The whole-message acknowledgement may have reached GitHub. Do not
        // poll, close, or retry this effect while its outcome is uncertain.
        BatchOutcome::AvailableOffersHeld
    } else {
        BatchOutcome::Stopped
    }
}
