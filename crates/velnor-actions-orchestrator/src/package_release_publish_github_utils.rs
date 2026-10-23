use super::{GitTagResponse, TagObservation, preflight};
use crate::{OrchestratorError, internal::internal};
use sha2::{Digest, Sha256};
use std::ffi::OsString;

pub(in crate::package_release_publish) const API_VERSION: &str = "2026-03-10";

pub(in crate::package_release_publish) fn graphql_arguments(
    query: &str,
    fields: &[(&str, String)],
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("graphql"),
        OsString::from("--raw-field"),
        OsString::from(format!("query={query}")),
    ];
    for (key, value) in fields {
        args.push(OsString::from("--field"));
        args.push(OsString::from(format!("{key}={value}")));
    }
    args
}

pub(in crate::package_release_publish) fn gh_api_arguments(
    request_args: Vec<OsString>,
    accept: &str,
) -> Vec<OsString> {
    let mut args = vec![OsString::from("api")];
    args.extend([
        OsString::from("-H"),
        OsString::from(format!("Accept: {accept}")),
        OsString::from("-H"),
        OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
    ]);
    args.extend(request_args);
    args
}

pub(in crate::package_release_publish) fn verify_tap_update_environment(
    repository: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    preflight::tap_update_environment(repository, branch)
}

pub(in crate::package_release_publish) fn annotated_tag_observation(
    expected_tag: &str,
    annotated: GitTagResponse,
) -> Result<TagObservation, OrchestratorError> {
    if annotated.tag != expected_tag
        || annotated.object.object_type != "commit"
        || !is_lowercase_git_sha(&annotated.sha)
        || !is_lowercase_git_sha(&annotated.object.sha)
    {
        return Err(internal("package_publish_tag_target_invalid"));
    }
    Ok(TagObservation {
        source_sha: annotated.object.sha,
        message: Some(annotated.message),
    })
}

pub(in crate::package_release_publish) fn is_lowercase_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(in crate::package_release_publish) fn sha256(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        push_hex_byte(&mut out, byte, false);
    }
    out
}

pub(in crate::package_release_publish) fn encode_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            push_hex_byte(&mut out, byte, true);
        }
    }
    out
}

fn push_hex_byte(output: &mut String, byte: u8, uppercase: bool) {
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    let hex = if uppercase { UPPER } else { LOWER };
    output.push(char::from(hex[usize::from(byte >> 4)]));
    output.push(char::from(hex[usize::from(byte & 0x0f)]));
}
