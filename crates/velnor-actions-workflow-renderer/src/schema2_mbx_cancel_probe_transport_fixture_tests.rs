//! Fake transport that asserts fixed hosts, redirect boundaries, and byte caps.

pub(super) const FAKE_CURL: &str = concat!(
    include_str!("schema2_mbx_cancel_probe_transport_fixture_a.inc"),
    include_str!("schema2_mbx_cancel_probe_transport_fixture_b.inc"),
);
