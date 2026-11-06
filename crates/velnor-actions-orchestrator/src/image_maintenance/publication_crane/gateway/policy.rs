// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use ureq::http::Uri;

fn clean(value: &str) -> bool {
    !value
        .bytes()
        .any(|byte| byte < 32 || byte == 127 || byte == b'\\')
}

fn decoded(value: &str) -> Option<Vec<u8>> {
    let mut result = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()?
        } else {
            byte
        };
        if byte < 32 || byte == 127 || byte == b'\\' {
            return None;
        }
        result.push(byte);
    }
    Some(result)
}

fn safe_path(path: &str) -> bool {
    let Some(path) = decoded(path) else {
        return false;
    };
    !path
        .split(|byte| *byte == b'/')
        .any(|part| part == b"." || part == b"..")
}

pub(super) fn safe_query(query: &str) -> bool {
    query.is_empty()
        || query.split('&').all(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            !key.is_empty() && decoded(key).is_some() && decoded(value).is_some()
        })
}

fn origin(uri: &Uri, host: &str) -> bool {
    uri.scheme_str() == Some("https")
        && uri.authority().is_some_and(|authority| {
            authority.as_str().eq_ignore_ascii_case(host)
                || authority
                    .as_str()
                    .eq_ignore_ascii_case(&format!("{host}:443"))
        })
}

pub(super) fn upload_location(base: &str, location: &str, repository: &str) -> Option<String> {
    if location.is_empty() || location.len() > 4096 || !clean(location) || location.contains('#') {
        return None;
    }
    let absolute = location.starts_with("//") || location.split(['/', '?']).next()?.contains(':');
    let target = if absolute {
        let absolute_url = if location.starts_with("//") {
            format!("https:{location}")
        } else {
            location.to_owned()
        };
        let parsed: Uri = absolute_url.parse().ok()?;
        if !origin(&parsed, "ghcr.io") || !safe_path(parsed.path()) {
            return None;
        }
        parsed.path_and_query()?.as_str().to_owned()
    } else {
        if !safe_path(location.split('?').next()?) {
            return None;
        }
        if location.starts_with('/') {
            location.to_owned()
        } else {
            let base_path = base.split('?').next()?;
            if location.starts_with('?') {
                format!("{base_path}{location}")
            } else {
                let parent = base_path.rsplit_once('/')?.0;
                format!("{parent}/{location}")
            }
        }
    };
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let decoded_path = decoded(path)?;
    let prefix_a = format!("/v2/{repository}/blobs/upload/");
    let prefix_b = format!("/v2/{repository}/blobs/uploads/");
    if !safe_path(path)
        || !(decoded_path.starts_with(prefix_a.as_bytes())
            || decoded_path.starts_with(prefix_b.as_bytes()))
        || !safe_query(query)
        || query.split('&').any(|pair| {
            decoded(pair.split('=').next().unwrap_or_default()).as_deref() == Some(b"digest")
        })
    {
        return None;
    }
    Some(target)
}

pub(super) fn cdn_location(location: &str, digest: &str) -> Option<String> {
    if !clean(location) || location.contains('#') {
        return None;
    }
    let uri: Uri = location.parse().ok()?;
    if !origin(&uri, "pkg-containers.githubusercontent.com")
        || !uri.path().ends_with(&format!("/blobs/{digest}"))
        || decoded(uri.path_and_query()?.as_str()).is_none()
    {
        return None;
    }
    Some(location.to_owned())
}
