//! Bounded HTTP range requests for stable manifests.

use std::time::Duration;

use crate::context::FETCH_CAP;

use super::{FETCH_TIMEOUT, fetch_http_with_agent_mode, upstream_agent};

pub(in crate::probe) fn fetch_prefix_text(url: &str, prefix_len: usize) -> Result<String, String> {
    fetch_http_prefix_with_agent(upstream_agent()?, url, FETCH_TIMEOUT, prefix_len)
}

pub(in crate::probe) fn fetch_http_prefix_with_agent(
    agent: &ureq::Agent,
    url: &str,
    timeout: Duration,
    prefix_len: usize,
) -> Result<String, String> {
    if prefix_len == 0 || prefix_len > FETCH_CAP {
        return Err(format!(
            "requested prefix must be between 1 and {FETCH_CAP} bytes"
        ));
    }
    fetch_http_with_agent_mode(agent, url, timeout, Some(prefix_len))
}

pub(super) fn validate_prefix_response(
    response: &ureq::http::Response<ureq::Body>,
    prefix_len: usize,
) -> Result<(), String> {
    if response.status().as_u16() != 206 {
        return Err(format!(
            "range request returned HTTP {}, expected 206 Partial Content",
            response.status().as_u16()
        ));
    }

    let mut encodings = response.headers().get_all("content-encoding").iter();
    if let Some(encoding) = encodings.next()
        && (encodings.next().is_some()
            || !encoding
                .to_str()
                .is_ok_and(|value| value.trim().eq_ignore_ascii_case("identity")))
    {
        return Err("range response must use identity Content-Encoding".to_owned());
    }

    let mut ranges = response.headers().get_all("content-range").iter();
    let range = ranges
        .next()
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "range response has no valid Content-Range".to_owned())?;
    if ranges.next().is_some() || !content_range_matches(range, prefix_len) {
        return Err("range response Content-Range does not match the requested prefix".to_owned());
    }
    Ok(())
}

fn content_range_matches(value: &str, prefix_len: usize) -> bool {
    let Some((unit, value)) = value.trim().split_once(' ') else {
        return false;
    };
    if unit != "bytes" {
        return false;
    }
    let Some((range, total)) = value.split_once('/') else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    let (Ok(start), Ok(end), Ok(total)) = (
        start.parse::<usize>(),
        end.parse::<usize>(),
        total.parse::<usize>(),
    ) else {
        return false;
    };
    start == 0 && end == prefix_len.saturating_sub(1) && total > end
}
