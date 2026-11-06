// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gateway_rejects_ambiguous_http_framing_before_external_io() -> Result<(), Box<dyn Error>> {
    let registry = Arc::new(Registry::default());
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let local = gateway.repository(REPOSITORY);
    let (address, repository) = local
        .split_once('/')
        .ok_or("gateway reference is invalid")?;
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    write!(
        stream,
        "POST /v2/{repository}/blobs/uploads/ HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n"
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;

    let state = registry.state.lock().map_err(lock_error)?;
    assert_eq!(state.token_requests, 0);
    assert_eq!(state.bad_requests, Vec::<String>::new());
    assert!(!response.starts_with(b"HTTP/1.1 2"));
    Ok(())
}

#[test]
fn gateway_returns_created_for_a_committed_blob_location() -> Result<(), Box<dyn Error>> {
    let registry = Arc::new(Registry::default());
    let fixture = Layout::new()?;
    registry
        .state
        .lock()
        .map_err(lock_error)?
        .uploads
        .insert("completed".into(), fixture.layer.clone());
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let response = raw_gateway(
        &gateway,
        "PUT",
        &format!("blobs/uploads/completed?digest={}", fixture.layer_digest),
    )?;
    assert!(
        response.starts_with("HTTP/1.1 201 "),
        "missing committed response: {response:?}"
    );
    assert_eq!(
        registry
            .state
            .lock()
            .map_err(lock_error)?
            .blobs
            .get(&fixture.layer_digest),
        Some(&fixture.layer)
    );
    Ok(())
}

#[test]
fn gateway_does_not_expose_an_upstream_authentication_realm() -> Result<(), Box<dyn Error>> {
    let registry = Arc::new(Registry {
        behavior: RegistryBehavior::Challenge,
        ..Registry::default()
    });
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let response = raw_gateway(&gateway, "GET", "manifests/fixture")?;
    let (token_requests, bad_requests) = {
        let state = registry.state.lock().map_err(lock_error)?;
        (state.token_requests, state.bad_requests.clone())
    };
    assert!(
        response.starts_with("HTTP/1.1 401 "),
        "{response:?}; token requests: {token_requests}; unexpected requests: {bad_requests:?}"
    );
    assert!(
        !response.to_ascii_lowercase().contains("www-authenticate:"),
        "foreign realm exposed: {response:?}"
    );
    assert_eq!(token_requests, 1);
    Ok(())
}

#[test]
fn gateway_rejects_foreign_and_repeated_cdn_redirects() -> Result<(), Box<dyn Error>> {
    let fixture = Layout::new()?;
    for registry in [
        Registry {
            behavior: RegistryBehavior::ForeignCdn,
            ..Registry::default()
        },
        Registry {
            behavior: RegistryBehavior::SecondCdnRedirect,
            ..Registry::default()
        },
    ] {
        let registry = Arc::new(registry);
        registry
            .state
            .lock()
            .map_err(lock_error)?
            .blobs
            .insert(fixture.layer_digest.clone(), fixture.layer.clone());
        let gateway = RegistryGateway::with_io(
            REPOSITORY,
            AUTHORIZATION.into(),
            registry.clone(),
            CheckDeadline::after(Duration::from_secs(90))?,
        )?;
        let reference = format!(
            "{}@{}",
            gateway.repository(REPOSITORY),
            fixture.layer_digest
        );

        let response = raw_gateway(&gateway, "GET", &format!("blobs/{}", fixture.layer_digest))?;
        assert!(
            !response.starts_with("HTTP/1.1 3"),
            "upstream redirect exposed: {response:?}"
        );
        assert!(crane(gateway.config().as_path(), &["blob", &reference]).is_err());
        assert_eq!(
            registry.state.lock().map_err(lock_error)?.bad_requests,
            Vec::<String>::new()
        );
    }
    Ok(())
}

pub(super) fn raw_gateway(
    gateway: &RegistryGateway,
    method: &str,
    path: &str,
) -> Result<String, Box<dyn Error>> {
    let local = gateway.repository(REPOSITORY);
    let (address, repository) = local.split_once('/').ok_or("bad reference")?;
    raw_gateway_address(address, method, &format!("/v2/{repository}/{path}"))
}

pub(super) fn raw_gateway_address(
    address: &str,
    method: &str,
    target: &str,
) -> Result<String, Box<dyn Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[test]
fn gateway_accepts_a_client_that_writes_after_connect() -> Result<(), Box<dyn Error>> {
    let registry = Arc::new(Registry::default());
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let local = gateway.repository(REPOSITORY);
    let (address, _) = local.split_once('/').ok_or("bad reference")?;
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    std::thread::sleep(Duration::from_millis(200));
    write!(
        stream,
        "GET /v2/ HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    assert!(
        response.starts_with("HTTP/1.1 200 "),
        "delayed client was dropped: {response:?}"
    );
    assert_eq!(registry.state.lock().map_err(lock_error)?.token_requests, 0);
    Ok(())
}
