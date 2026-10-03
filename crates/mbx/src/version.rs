//! Source identity shared by the CLI, agent handshake, and build reports.

/// Official source version on which this build is based.
pub const SOURCE_BASE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exact release identity of the compiled source and feature profile.
#[cfg(feature = "owned-cache-transport")]
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "-owned-cache-transport");

/// Default builds retain the official package identity.
#[cfg(not(feature = "owned-cache-transport"))]
pub const VERSION: &str = SOURCE_BASE_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_base_is_the_package_version() {
        assert_eq!(SOURCE_BASE_VERSION, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn release_identity_matches_the_compiled_feature() {
        #[cfg(feature = "owned-cache-transport")]
        assert_eq!(VERSION, "1.21.1-owned-cache-transport");
        #[cfg(not(feature = "owned-cache-transport"))]
        assert_eq!(VERSION, SOURCE_BASE_VERSION);
    }

    #[test]
    fn session_event_records_the_compiled_release_identity() {
        use crate::events::{EventWriter, SessionEvent, parse_events, session_paths};
        let store = tempfile::tempdir().unwrap();
        let writer = EventWriter::with_limit(store.path(), None);
        writer.started(std::path::Path::new("/workspace"), &["check".into()], None);
        let contents =
            std::fs::read_to_string(session_paths(store.path(), writer.id()).events).unwrap();
        let events = parse_events(&contents);
        assert!(matches!(
            &events[..],
            [SessionEvent::SessionStarted { mbx_version, .. }] if mbx_version == VERSION
        ));
    }
}
