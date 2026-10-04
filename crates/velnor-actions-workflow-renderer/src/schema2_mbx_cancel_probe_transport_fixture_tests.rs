//! Fake transport that asserts fixed hosts, redirect boundaries, and byte caps.

pub(super) const FAKE_CURL: &str = concat!(
    include_str!("schema2_mbx_cancel_probe_transport_fixture_preamble.sh"),
    include_str!("schema2_mbx_cancel_probe_transport_fixture_cache_artifacts.sh"),
);
