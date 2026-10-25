use super::*;
use crate::journal::StorageRole;

pub(super) fn assert_failure_error(
    failure: &PreparationFailure,
    role: StorageRole,
    point: FailurePoint,
) -> Result<(), String> {
    if failure.error.is_empty() {
        return Err(format!(
            "The injected {} failure had no error.",
            role.as_str()
        ));
    }
    if matches!(point, FailurePoint::ExistingOwnedVolume) && !failure.ownership_error {
        return Err(format!(
            "Production preparation did not return `HostError::Ownership` for the existing {} volume: {}.",
            role.as_str(),
            failure.error
        ));
    }
    Ok(())
}

pub(super) fn assert_failure_response(
    captured: &CapturedApi,
    role: StorageRole,
    expected_volume: Option<&str>,
    point: FailurePoint,
) -> Result<(), String> {
    match point {
        FailurePoint::JournalClaim if captured.server_error.is_some() => {
            return Err(format!(
                "The Docker fixture failed: {:?}.",
                captured.server_error
            ));
        }
        FailurePoint::DockerInspect
            if !captured
                .server_error
                .as_deref()
                .is_some_and(|error| error.contains("volume inspection transport failure")) =>
        {
            return Err(format!(
                "The {} volume inspection failure was not observed.",
                role.as_str()
            ));
        }
        FailurePoint::ExistingOwnedVolume => {
            assert_inspection_status(captured, role, expected_volume, 200)?;
        }
        FailurePoint::InspectStatus(status) => {
            assert_inspection_status(captured, role, expected_volume, status)?;
        }
        _ => {}
    }
    Ok(())
}

fn assert_inspection_status(
    captured: &CapturedApi,
    role: StorageRole,
    expected_volume: Option<&str>,
    status: u16,
) -> Result<(), String> {
    let Some(name) = expected_volume else {
        return Err(format!(
            "The {} fixture omitted its volume name.",
            role.as_str()
        ));
    };
    if captured.requests.iter().any(|request| {
        request.method == "GET"
            && request.target.contains(name)
            && request.response_status == Some(status)
    }) {
        return Ok(());
    }
    Err(format!(
        "The {} volume inspection did not return HTTP {status}.",
        role.as_str()
    ))
}

pub(super) fn assert_created_prefix(
    captured: &CapturedApi,
    role: StorageRole,
    expected_prefix: &[&str],
) -> Result<(), String> {
    let created = captured
        .requests
        .iter()
        .filter(|request| request.method == "POST" && request.target == "/volumes/create")
        .map(|request| {
            let body: serde_json::Value =
                serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
            body["Labels"]["velnor.role"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "A storage create omitted its role label.".to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?;
    if created
        .iter()
        .map(String::as_str)
        .eq(expected_prefix.iter().copied())
    {
        return Ok(());
    }
    Err(format!(
        "The failed {} operation created unexpected volumes: {created:?}; expected prefix: {expected_prefix:?}.",
        role.as_str()
    ))
}

pub(super) fn assert_no_container_create(captured: &CapturedApi) -> Result<(), String> {
    if captured
        .requests
        .iter()
        .any(|request| request.method == "POST" && request.target.starts_with("/containers/create"))
    {
        return Err("Storage initialization continued after the volume failure.".to_owned());
    }
    Ok(())
}
