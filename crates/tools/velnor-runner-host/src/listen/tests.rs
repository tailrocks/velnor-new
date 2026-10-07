//! Queue path. No network.

use crate::error::HostError;
use crate::listen::{absolute_https, queue_target};
use crate::queue_path;

#[test]
fn queue_path_strips_the_admin_origin() {
    let base = "https://pipelines.example.test/tenant";
    let absolute =
        "https://pipelines.example.test/tenant/_apis/runtime/runnerscalesets/1/sessions/s/messages";
    assert_eq!(
        queue_path(base, absolute),
        Some("_apis/runtime/runnerscalesets/1/sessions/s/messages")
    );
    assert_eq!(
        queue_path(base, "_apis/runtime/runnerscalesets/1/sessions/s/messages"),
        Some("_apis/runtime/runnerscalesets/1/sessions/s/messages")
    );
    assert_eq!(
        queue_path(base, "https://other.example.test/_apis/messages"),
        None
    );
    assert_eq!(queue_path(base, ""), None);
}

#[test]
fn absolute_https_splits_origin_and_path() -> Result<(), HostError> {
    let split =
        absolute_https("https://broker.example.test/v1/messages").ok_or(HostError::Endpoint)?;
    assert_eq!(split.origin, "https://broker.example.test");
    assert_eq!(split.path, "v1/messages");
    assert!(absolute_https("http://broker.example.test/v1").is_none());
    assert!(absolute_https("https://user@broker.example.test/v1").is_none());
    Ok(())
}

#[test]
fn queue_urls_reject_ambiguous_authority_and_path_values() {
    for url in [
        "https://broker.example.test/v1?token=x",
        "https://broker.example.test/v1#fragment",
        "https://broker.example.test/v1 path",
        "https://broker.example.test/v1\npath",
        "https://broker.example.test%2f.attacker.test/v1",
        "https://broker.example.test:abc/v1",
        "https://[not-ipv6]/v1",
        "https://broker.example.test/%2e%2e/admin",
        "https://broker.example.test/v1/%2Fadmin",
    ] {
        assert!(absolute_https(url).is_none(), "accepted unsafe URL {url:?}");
    }
}

#[test]
fn queue_url_can_change_origin_but_relative_paths_keep_the_admin_origin() {
    let admin = "https://admin.example.test";
    let absolute = queue_target(admin, "https://queue.example.test/_apis/runtime/messages");
    assert_eq!(
        absolute,
        Some(crate::listen::Absolute {
            origin: "https://queue.example.test".to_owned(),
            path: "_apis/runtime/messages".to_owned(),
        })
    );
    let relative = queue_target(admin, "/_apis/runtime/messages");
    assert_eq!(
        relative,
        Some(crate::listen::Absolute {
            origin: admin.to_owned(),
            path: "_apis/runtime/messages".to_owned(),
        })
    );
}
