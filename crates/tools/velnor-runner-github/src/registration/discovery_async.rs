//! Native async API for durable, one-shot discovery credential bootstrap.
//!
//! The reviewed synchronous API remains available for existing bounded host
//! callers. Async hosts should use this module so journal I/O can be awaited
//! without a nested runtime or blocking the executor thread.

mod bootstrap;
mod intent;
mod transport;

pub use bootstrap::{
    exchange_organization_discovery_admin_once_async,
    exchange_repository_discovery_admin_once_async, issue_organization_discovery_token_async,
    issue_repository_discovery_token_async, organization_admin_evidence,
    read_repository_admin_evidence_async,
};
pub use intent::{
    AsyncDiscoveryIntentStore, AsyncScopedDiscoveryIntentStore, DiscoveryStoreFuture,
};
pub(crate) use transport::execute_discovery;
pub use transport::{AsyncDiscoveryTransport, DiscoveryExchange};
