//! Validation for fixed checksummed HTTP tool specs used by Mise.

/// Validate Mise's fixed `http:<name>[url=...,checksum=sha256:...]@<version>` form.
pub(crate) fn is_verified_http_tool_spec(value: &str) -> bool {
    let Some((name, rest)) = value.split_once("[url=") else {
        return false;
    };
    let Some((url, rest)) = rest.split_once(",checksum=sha256:") else {
        return false;
    };
    let Some((checksum, version)) = rest.split_once("]@") else {
        return false;
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && url.starts_with("https://")
        && url
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'.' | b'-' | b'_'))
        && checksum.len() == 64
        && checksum.bytes().all(|b| b.is_ascii_hexdigit())
        && !version.is_empty()
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}
