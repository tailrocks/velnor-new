use std::time::Duration;

use http_body_util::Empty;
use hyper::body::Bytes;
use hyper::{Method, Request};
use tokio::net::UnixStream;
use tokio::time::Instant;

use super::{FixtureServer, accept_bounded, read_request, respond_info, socket_path};

#[tokio::test]
async fn expiry_after_valid_info_prevents_original_request_dispatch() -> Result<(), String> {
    let (_directory, endpoint, listener) = socket_path()?;
    let server = FixtureServer::spawn(async move {
        let (mut stream, _) = accept_bounded(&listener).await?;
        let info = read_request(&mut stream)
            .await?
            .ok_or("missing info request")?;
        respond_info(&mut stream, "engine-A").await?;
        let original = read_request(&mut stream).await?;
        Ok::<_, String>((info, original))
    });

    let stream = UnixStream::connect(&endpoint)
        .await
        .map_err(|error| error.to_string())?;
    let (mut sender, connection) =
        super::super::http1::handshake(super::super::TokioIo::new(stream))
            .await
            .map_err(|error| error.to_string())?;
    let driver = super::super::DriverGuard::new(tokio::spawn(async move {
        drop(connection.with_upgrades().await);
    }));
    let deadline = Instant::now() + Duration::from_secs(2);
    let response = super::super::send_before_deadline(
        &mut sender,
        super::super::info_request(None, Empty::<Bytes>::new())
            .map_err(|_| "info request build")?,
        deadline,
    )
    .await
    .map_err(|_| "initial info was not sent")?;
    assert_eq!(
        super::super::parse_info_response(response, deadline)
            .await
            .map_err(|_| "valid info response was rejected")?,
        "engine-A"
    );

    let request = Request::builder()
        .method(Method::POST)
        .uri("http://localhost/v1.53/containers/create")
        .body(Empty::<Bytes>::new())
        .map_err(|error| error.to_string())?;
    let expired = Instant::now() - Duration::from_millis(1);
    assert!(
        super::super::send_before_deadline(&mut sender, request, expired)
            .await
            .is_err()
    );
    drop(driver);

    let (info, original) = server.finish().await??;
    assert_eq!(info, "GET /info HTTP/1.1");
    assert_eq!(original, None);
    Ok(())
}
