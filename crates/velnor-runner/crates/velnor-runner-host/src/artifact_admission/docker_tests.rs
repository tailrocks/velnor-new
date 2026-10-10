use std::sync::atomic::{AtomicBool, Ordering};

use bollard::{BollardRequest, ClientVersion};

use super::{
    INSPECT_PATH, MINIMUM_PLATFORM_API, PLATFORM_QUERY, add_platform_query, ensure_platform_api,
    is_image_missing, versioned_inspect_path,
};
use crate::error::HostError;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestMarker(u8);

fn version(major_version: usize, minor_version: usize) -> ClientVersion {
    ClientVersion {
        major_version,
        minor_version,
    }
}

fn request(method: &str, uri: &str) -> Result<BollardRequest, HostError> {
    let mut request = BollardRequest::new(bollard::body_full(Vec::<u8>::new().into()));
    *request.method_mut() = method.parse().map_err(|_| HostError::Docker)?;
    *request.uri_mut() = uri.parse().map_err(|_| HostError::Docker)?;
    request
        .headers_mut()
        .insert("x-preserved", "yes".parse().map_err(|_| HostError::Docker)?);
    Ok(request)
}

#[test]
fn enforces_the_minimum_platform_query_api_version() {
    let below = version(
        MINIMUM_PLATFORM_API.0,
        MINIMUM_PLATFORM_API.1.saturating_sub(1),
    );
    assert_eq!(ensure_platform_api(below), Err(HostError::Identity));
    assert_eq!(ensure_platform_api(version(1, 49)), Ok(()));
    assert_eq!(ensure_platform_api(version(2, 0)), Ok(()));
}

#[test]
fn only_exact_docker_not_found_allows_an_import_attempt() {
    let not_found = bollard::errors::Error::DockerResponseServerError {
        status_code: 404,
        message: "not found".to_owned(),
    };
    let server_error = bollard::errors::Error::DockerResponseServerError {
        status_code: 500,
        message: "server error".to_owned(),
    };

    assert!(is_image_missing(&not_found));
    assert!(!is_image_missing(&server_error));
}

#[test]
fn versions_the_exact_unversioned_get_and_adds_json_encoded_platform() -> Result<(), HostError> {
    let expected_path = versioned_inspect_path(version(1, 53));
    assert_eq!(expected_path, format!("/v1.53{INSPECT_PATH}"));
    let mut request = request(
        "GET",
        "unix://socket/images/velnor-resource-probe:linux-amd64/json",
    )?;
    request.extensions_mut().insert(RequestMarker(7));
    let applied = AtomicBool::new(false);

    let request = add_platform_query(request, INSPECT_PATH, &expected_path, &applied);

    assert!(applied.load(Ordering::Acquire));
    assert_eq!(
        request.uri().to_string(),
        format!("unix://socket{expected_path}?platform={PLATFORM_QUERY}")
    );
    assert_eq!(request.method().as_str(), "GET");
    assert_eq!(format!("{:?}", request.version()), "HTTP/1.1");
    assert_eq!(
        request
            .headers()
            .get("x-preserved")
            .and_then(|v| v.to_str().ok()),
        Some("yes")
    );
    assert_eq!(
        request.extensions().get::<RequestMarker>(),
        Some(&RequestMarker(7))
    );
    Ok(())
}

#[test]
fn leaves_unrelated_or_prequeried_requests_unchanged() -> Result<(), HostError> {
    let versioned_path = versioned_inspect_path(version(1, 53));
    for (method, uri) in [
        ("GET", "unix://socket/images/velnor-other:latest/json"),
        (
            "POST",
            "unix://socket/images/velnor-resource-probe:linux-amd64/json",
        ),
        (
            "GET",
            "unix://socket/images/velnor-resource-probe:linux-amd64/json?platform=caller",
        ),
    ] {
        let request = request(method, uri)?;
        let original = request.uri().to_string();
        let applied = AtomicBool::new(false);

        let request = add_platform_query(request, INSPECT_PATH, &versioned_path, &applied);

        assert!(!applied.load(Ordering::Acquire));
        assert_eq!(request.uri().to_string(), original);
        assert_eq!(request.method().as_str(), method);
        assert_eq!(
            request
                .headers()
                .get("x-preserved")
                .and_then(|v| v.to_str().ok()),
            Some("yes")
        );
    }
    Ok(())
}

#[test]
fn versioned_path_or_unexpected_query_cannot_become_an_applied_platform_request()
-> Result<(), HostError> {
    let versioned_path = versioned_inspect_path(version(1, 53));
    for uri in [
        "unix://socket/v1.53/images/velnor-resource-probe:linux-amd64/json",
        "unix://socket/images/velnor-resource-probe:linux-amd64/json?platform=caller",
    ] {
        let request = request("GET", uri)?;
        let original = request.uri().to_string();
        let applied = AtomicBool::new(false);

        let request = add_platform_query(request, INSPECT_PATH, &versioned_path, &applied);

        assert!(!applied.load(Ordering::Acquire));
        assert_eq!(request.uri().to_string(), original);
    }
    Ok(())
}
