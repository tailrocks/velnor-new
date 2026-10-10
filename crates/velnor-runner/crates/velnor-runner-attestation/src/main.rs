//! Fixed-protocol, verification-only attestation helper.

mod root_chain;
mod tuf_state;
mod verifier;

#[cfg(test)]
#[path = "tuf_cache_rollback_tests.rs"]
mod tuf_cache_rollback_tests;
#[cfg(test)]
#[path = "tuf_cache_tests.rs"]
mod tuf_cache_tests;
#[cfg(test)]
#[path = "tuf_fixture_tests.rs"]
mod tuf_fixture_tests;

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use verifier::{InlineVerifyRequest, verify_inline_checksum_target};

const PLATFORM_ENV_KEY: &str = "__CF_USER_TEXT_ENCODING";

fn validate_platform_environment() -> Result<(), ()> {
    let mut platform_key_seen = false;
    for (key, _) in std::env::vars_os() {
        if key != PLATFORM_ENV_KEY || platform_key_seen {
            return Err(());
        }
        platform_key_seen = true;
    }
    Ok(())
}

fn read_bounded_request() -> Result<Vec<u8>, ()> {
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .take(verifier::MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|_| ())?;
    if input.len() > verifier::MAX_REQUEST_BYTES {
        return Err(());
    }
    Ok(input)
}

async fn run() -> Result<(), ()> {
    if std::env::args_os().len() != 1 {
        return Err(());
    }
    validate_platform_environment()?;
    let input = read_bounded_request()?;
    let request: InlineVerifyRequest = serde_json::from_slice(&input).map_err(|_| ())?;
    let deadline = Instant::now() + verifier::VERIFY_DEADLINE;
    let response = tokio::time::timeout(
        verifier::VERIFY_DEADLINE,
        verify_inline_checksum_target(request, deadline),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let response = serde_json::to_vec(&response).map_err(|_| ())?;
    if response.len() > verifier::MAX_RESPONSE_BYTES {
        return Err(());
    }
    io::stdout().lock().write_all(&response).map_err(|_| ())?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}
