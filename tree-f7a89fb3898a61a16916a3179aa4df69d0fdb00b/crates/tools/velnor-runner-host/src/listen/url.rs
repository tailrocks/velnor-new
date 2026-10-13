//! Validate and split message-queue URLs before routing queue credentials.

/// Split `https` URL into its origin and relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Absolute {
    /// `https://host` without path.
    pub origin: String,
    /// Path without leading slash.
    pub path: String,
}

/// Split an `https` URL into origin and path. Rejects anything else.
#[must_use]
pub fn absolute_https(url: &str) -> Option<Absolute> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !safe_host(host) || !safe_path(path) {
        return None;
    }
    Some(Absolute {
        origin: format!("https://{host}"),
        path: path.to_owned(),
    })
}

/// Path relative to the admin origin. Absolute URLs outside `base` are rejected.
#[must_use]
pub fn queue_path<'a>(base: &str, queue_url: &'a str) -> Option<&'a str> {
    let relative = if let Some(path) = queue_url.strip_prefix(base) {
        path.strip_prefix('/')?
    } else {
        if queue_url.contains("://") {
            return None;
        }
        queue_url.trim_start_matches('/')
    };
    (safe_path(relative) && !relative.contains("://")).then_some(relative)
}

pub(crate) fn queue_target(default_origin: &str, url: &str) -> Option<Absolute> {
    if url.starts_with("https://") {
        return absolute_https(url);
    }
    if url.contains("://") || !safe_path(url.trim_start_matches('/')) {
        return None;
    }
    Some(Absolute {
        origin: default_origin.to_owned(),
        path: url.trim_start_matches('/').to_owned(),
    })
}

fn safe_host(host: &str) -> bool {
    if host.is_empty() || host.bytes().any(|byte| !byte.is_ascii()) {
        return false;
    }
    let Some((hostname, port)) = host_port(host) else {
        return false;
    };
    if port.is_some_and(|value| value.parse::<u16>().map_or(true, |port| port == 0)) {
        return false;
    }
    if hostname.starts_with('[') {
        return hostname
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .is_some_and(|value| value.parse::<std::net::Ipv6Addr>().is_ok());
    }
    hostname.len() <= 253
        && hostname.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.bytes().all(safe_path_byte)
                && !has_encoded_path_separator(segment)
        })
}

fn safe_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'%'
        )
}

fn has_encoded_path_separator(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(pair) = bytes.get(index + 1..index + 3) else {
                return true;
            };
            let decoded = [pair[0].to_ascii_lowercase(), pair[1].to_ascii_lowercase()];
            if !pair.iter().all(u8::is_ascii_hexdigit)
                || matches!(decoded, [b'2', b'f' | b'e'] | [b'5', b'c'])
            {
                return true;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    false
}

fn host_port(host: &str) -> Option<(&str, Option<&str>)> {
    if host.starts_with('[') {
        let close = host.find(']')?;
        let name = host.get(..=close)?;
        let tail = host.get(close + 1..)?;
        let port = if tail.is_empty() {
            None
        } else {
            Some(tail.strip_prefix(':')?)
        };
        if port.is_some_and(str::is_empty) {
            return None;
        }
        return Some((name, port));
    }
    if host.matches(':').count() > 1 {
        return None;
    }
    match host.split_once(':') {
        Some((name, port)) if !port.is_empty() => Some((name, Some(port))),
        Some(_) => None,
        None => Some((host, None)),
    }
}
