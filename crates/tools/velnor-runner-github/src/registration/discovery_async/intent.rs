use std::{future::Future, pin::Pin};

use crate::SessionError;

use super::super::{DiscoveryCredentialOutcome, DiscoveryCredentialStep, DiscoveryIntentId};

/// Boxed native-async journal result. The lifetime ties the future to the
/// mutable journal borrow; `Send` lets hosts await Turso without blocking an
/// executor thread.
pub type DiscoveryStoreFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, SessionError>> + Send + 'a>>;

/// Async durable intent store for the two credential POSTs.
///
/// `persist_before` must atomically refuse a step with an unresolved prior
/// intent for the same destination and repository. It returns only after the
/// new Pending intent is durable. If that future is cancelled or errors, the
/// caller sends no request. `record_outcome` may transition only that exact
/// Pending row; cancellation or failure after POST leaves it unresolved.
/// Neither method may persist credential values or response bodies.
pub trait AsyncDiscoveryIntentStore: Send {
    /// Durably reserve exactly one operation before its external POST.
    fn persist_before<'a>(
        &'a mut self,
        step: DiscoveryCredentialStep,
        repository_id: i64,
        full_name: &'a str,
    ) -> DiscoveryStoreFuture<'a, DiscoveryIntentId>;

    /// Record the result observed for the same operation.
    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: DiscoveryCredentialOutcome,
    ) -> DiscoveryStoreFuture<'_, ()>;
}
