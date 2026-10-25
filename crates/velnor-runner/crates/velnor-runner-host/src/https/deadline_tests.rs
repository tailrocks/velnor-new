use std::path::Path;
use std::time::{Duration, Instant};

use velnor_runner_github::{Method, SessionRequest};

use super::{HttpsTransport, curl_config};

#[test]
fn bounded_transport_keeps_the_callers_absolute_cutoff() {
    let deadline = Instant::now() + Duration::from_secs(30);
    let transport =
        HttpsTransport::with_deadline("https://api.github.com", deadline).expect("valid endpoint");

    assert_eq!(transport.deadline, Some(deadline));
}

#[test]
fn derived_transport_preserves_endpoint_and_never_extends_deadline() {
    let earlier = Instant::now() + Duration::from_secs(10);
    let later = earlier + Duration::from_secs(10);
    let transport =
        HttpsTransport::with_deadline("https://api.github.com", earlier).expect("valid endpoint");
    let bounded = transport.bounded_by_deadline(later);

    assert_eq!(bounded.base, transport.base);
    assert_eq!(bounded.deadline, Some(earlier));
    assert_eq!(bounded.max_time_seconds, transport.max_time_seconds);

    let tighter = earlier
        .checked_sub(Duration::from_secs(1))
        .expect("the test deadline is in the future");
    let bounded = bounded.bounded_by_deadline(tighter);
    assert_eq!(bounded.deadline, Some(tighter));
    assert_eq!(bounded.base, transport.base);

    let unrestricted = HttpsTransport::new("https://api.github.com").expect("valid endpoint");
    let bounded = unrestricted.bounded_by_deadline(earlier);
    assert_eq!(bounded.base, unrestricted.base);
    assert_eq!(bounded.deadline, Some(earlier));
}

#[test]
fn curl_config_uses_the_transport_timeout() -> Result<(), &'static str> {
    let transport =
        HttpsTransport::new("https://api.github.com").map_err(|_| "invalid HTTPS endpoint")?;
    let request = SessionRequest {
        method: Method::Get,
        path: "/rate_limit".to_owned(),
        query: None,
        headers: Vec::new(),
        body: Vec::new(),
    };
    let config = curl_config(
        "https://api.github.com/rate_limit",
        &request,
        Path::new("/tmp/request-body"),
        Duration::from_secs(transport.max_time_seconds),
    )
    .map_err(|_| "invalid curl configuration")?;

    assert!(config.lines().any(|line| line == "max-time = 60.000"));
    assert!(config.lines().any(|line| line == "output = \"-\""));
    Ok(())
}
