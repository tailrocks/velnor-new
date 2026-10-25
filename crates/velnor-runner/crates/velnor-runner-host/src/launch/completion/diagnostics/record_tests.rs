use serde_json::json;

use super::{RecordIdentity, diagnostic_record_fields};

const CONTAINER_ID: &str = "owned-container";
const CONTAINER_NAME: &str = "owned-runner";

#[test]
fn record_keeps_only_valid_image_digest_and_rfc3339_times() -> Result<(), String> {
    let record = diagnostic_record_fields(
        inspect(
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "2026-10-21T12:34:56.123456789Z",
            "2026-10-21T12:35:01+07:00",
        )
        .map_err(|error| error.to_string())?,
        &identity(),
    )
    .map_err(|error| error.to_string())?;
    let state = record
        .state
        .as_ref()
        .ok_or("The diagnostic record has no state.")?;
    if record.image.as_deref()
        != Some("sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        || state.started_at.as_deref() != Some("2026-10-21T12:34:56.123456789Z")
        || state.finished_at.as_deref() != Some("2026-10-21T12:35:01+07:00")
    {
        return Err("The diagnostic record omitted a valid image digest or timestamp.".to_owned());
    }
    Ok(())
}

#[test]
fn record_omits_untrusted_image_and_timestamp_strings() -> Result<(), String> {
    let record = diagnostic_record_fields(
        inspect(
            "not-a-digest-canary",
            "2026-13-40T99:88:77Z",
            "timestamp-canary",
        )
        .map_err(|error| error.to_string())?,
        &identity(),
    )
    .map_err(|error| error.to_string())?;
    let state = record
        .state
        .as_ref()
        .ok_or("The diagnostic record has no state.")?;
    let text = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    if record.image.is_some()
        || state.started_at.is_some()
        || state.finished_at.is_some()
        || text.contains("canary")
    {
        return Err("The diagnostic record contains untrusted image or timestamp text.".to_owned());
    }
    Ok(())
}

fn inspect(
    image: &str,
    started_at: &str,
    finished_at: &str,
) -> Result<bollard::models::ContainerInspectResponse, serde_json::Error> {
    serde_json::from_value(json!({
        "Id": CONTAINER_ID,
        "Name": format!("/{CONTAINER_NAME}"),
        "Image": image,
        "State": {"Status":"exited", "StartedAt":started_at, "FinishedAt":finished_at}
    }))
}

fn identity() -> RecordIdentity<'static> {
    RecordIdentity {
        connection_id: "connection".to_owned(),
        intent_id: 1,
        generation: 1,
        role: "runner",
        expected_name: CONTAINER_NAME,
        container_id: CONTAINER_ID,
    }
}
