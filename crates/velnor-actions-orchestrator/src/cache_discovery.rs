// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "cache_discovery/io.rs"]
mod io;
#[path = "cache_discovery/selection.rs"]
mod selection;
#[path = "cache_discovery/types.rs"]
mod types;

// Bridge re-exports for the orchestrator integration suites, which path-include
// this module. The lib owner (cache_keys T24 wiring) does not consume them yet,
// so they stay gated on test builds until the emission owner wires them.
#[cfg(test)]
pub use io::{
    RestoreObservation, discovery_canonical_bytes, discovery_digest, parse_discovery_bytes,
    verify_restore,
};

#[cfg(test)]
pub use selection::{
    classify_shadow, decide_discovery, primary_prefix, select_exact, select_predecessor,
};

#[cfg(test)]
pub use types::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, SelectableEntry, ShadowState,
};

#[cfg(test)]
mod bridge_smoke {
    use super::{
        DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
        ShadowState, classify_shadow, decide_discovery, discovery_canonical_bytes,
        discovery_digest, parse_discovery_bytes, primary_prefix, select_exact, select_predecessor,
        verify_restore,
    };

    fn consume<T>() {}

    #[test]
    fn bridge_reexports_resolve() {
        consume::<DiscoveryDecision>();
        consume::<DiscoveryRequest>();
        consume::<EntryProvenance>();
        consume::<RestoreObservation>();
        consume::<SelectableEntry>();
        consume::<ShadowState>();
        let _ = classify_shadow;
        let _ = decide_discovery;
        let _ = discovery_canonical_bytes;
        let _ = discovery_digest;
        let _ = parse_discovery_bytes;
        let _ = primary_prefix;
        let _ = select_exact;
        let _ = select_predecessor;
        let _ = verify_restore;
    }
}
