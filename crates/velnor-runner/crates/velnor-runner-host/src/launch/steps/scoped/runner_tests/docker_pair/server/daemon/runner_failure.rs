use super::super::{CapturedRequest, route};
use serde_json::{Value, json};

pub(super) fn create_rejection(
    status: Option<u16>,
    request: &CapturedRequest,
) -> Option<(u16, Vec<u8>)> {
    let status = status?;
    let body = serde_json::from_slice::<Value>(&request.body).ok()?;
    (request.method == "POST"
        && route(request) == "/containers/create"
        && body["Labels"]["io.velnor.role"].as_str() == Some("runner"))
    .then(|| {
        (
            status,
            json!({"message":"Scripted Runner create rejection."})
                .to_string()
                .into_bytes(),
        )
    })
}
