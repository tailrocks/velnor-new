use std::time::{Duration, Instant};

use velnor_runner_github::{Exchange, Method, SessionRequest};

use super::{CurlFail, Scratch, perform_with_scratch};

pub(crate) fn exchange_fixture_response(
    base: &str,
    max_response_bytes: usize,
) -> Result<Exchange, &'static str> {
    let request = SessionRequest {
        method: Method::Get,
        path: String::new(),
        query: None,
        headers: Vec::new(),
        body: Vec::new(),
    };
    let scratch = Scratch::create()
        .map_err(|_| "The test could not create its private request directory.")?;
    let path = scratch.dir.clone();
    let result = perform_with_scratch(
        base,
        Instant::now() + Duration::from_secs(5),
        &request,
        max_response_bytes,
        &scratch,
    );
    drop(scratch);
    if path.exists() {
        return Err("The private request directory still exists.");
    }
    result.map_err(|error| match error {
        CurlFail::ResponseTooLarge => "The response is larger than its byte limit.",
        CurlFail::Timeout => "The request did not finish before its time limit.",
        CurlFail::Reset => "The request failed.",
    })
}
