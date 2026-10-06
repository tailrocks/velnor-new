//! `POST .../generatejitconfig`. The request JSON and the response are not logged.

use serde::{Deserialize, Serialize};
use velnor_runner_core::RUNNER_WORK_FOLDER;
use zeroize::Zeroize;

use crate::refresh::{StatusClass, classify_status};
use crate::{EncodedJit, JitResult, RunnerReference, WireError, jit_path};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, bearer, execute, fresh_gate, json_content, user_agent};

/// JSON body for [`jit`]. `workFolder` is `_work`, relative to the runner root.
/// This matches the official Actions runner's work directory and the container
/// path mounted from this worker's named work volume. The JIT payload stays in
/// a private `/tmp` file and is not persisted in that volume.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] when `name` is empty or has
/// whitespace or a slash. The name is a container name, not a path.
pub fn jit_request(name: &str) -> Result<Vec<u8>, WireError> {
    if name.is_empty() || name.chars().any(|ch| ch.is_whitespace() || ch == '/') {
        return Err(WireError::RegistrationRejected);
    }
    serde_json::to_vec(&JitRequest {
        name,
        work_folder: RUNNER_WORK_FOLDER,
    })
    .map_err(|_| WireError::Encode)
}

/// `POST` `request_json` to the JIT route. `admin_token` is the admin bearer.
///
/// The 200 body is JSON. Both the encoded configuration and created runner
/// reference are preserved. Neither the request bytes nor the configuration
/// are written into [`std::fmt::Debug`] or [`std::fmt::Display`]. This call
/// does not refresh on HTTP 401.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty admin token,
/// [`SessionError::Uncertain`] on timeout, reset, server failure, or when a
/// success response lacks a usable `encodedJITConfig` or runner reference,
/// since the service may already have created the runner. Returns
/// [`SessionError::Wire`] for other rejected statuses and
/// [`SessionError::Conflict`] on HTTP 409.
pub fn jit<T>(
    transport: &mut T,
    scale_set_id: i64,
    admin_token: &str,
    request_json: &[u8],
) -> Result<JitResult, SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Post,
        path: jit_path(scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(admin_token)?, user_agent()],
        body: request_json.to_vec(),
    };
    let exchange = execute(transport, &request)?;
    require_ok(exchange.status)?;
    decode_jit(&exchange.body).map_err(|_| SessionError::Uncertain)
}

fn decode_jit(body: &[u8]) -> Result<JitResult, SessionError> {
    let parsed: JitBody = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let Some(runner) = parsed.runner else {
        return Err(SessionError::Wire(WireError::Malformed));
    };
    if parsed.encoded_jit_config.as_str().is_empty() {
        return Err(SessionError::Wire(WireError::Malformed));
    }
    Ok(JitResult {
        encoded_jit_config: EncodedJit::new(parsed.encoded_jit_config.into_string()),
        runner,
    })
}

#[derive(Serialize)]
struct JitRequest<'a> {
    name: &'a str,
    #[serde(rename = "workFolder")]
    work_folder: &'a str,
}

#[derive(Deserialize)]
struct JitBody {
    #[serde(rename = "encodedJITConfig")]
    encoded_jit_config: SecretJit,
    runner: Option<RunnerReference>,
}

#[derive(Deserialize)]
#[serde(transparent)]
struct SecretJit(String);

impl SecretJit {
    fn as_str(&self) -> &str {
        &self.0
    }

    fn into_string(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl Drop for SecretJit {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn require_ok(status: u16) -> Result<(), SessionError> {
    if status >= 500 || ((200..300).contains(&status) && status != 200) {
        return Err(SessionError::Uncertain);
    }
    match classify_status(status, &fresh_gate()) {
        Ok(StatusClass::Ok) => Ok(()),
        Ok(class) => Err(reject(class)),
        Err(error) => Err(SessionError::from(error)),
    }
}
