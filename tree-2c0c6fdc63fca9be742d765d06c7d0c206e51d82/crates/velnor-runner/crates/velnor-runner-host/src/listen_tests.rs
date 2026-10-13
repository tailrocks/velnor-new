//! Queue path. No network.

use crate::error::HostError;
use crate::listen::absolute_https;
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
