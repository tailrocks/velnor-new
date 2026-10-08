//! Validated message-queue path and query carried by the Actions session.

use std::fmt;

use zeroize::Zeroize;

use crate::WireError;

/// Parsed path and raw query from a host-validated `MessageQueueURL`.
///
/// The host transport validates and binds the complete absolute URL before it
/// constructs this value. The query may carry routing credentials and is never
/// included in Debug output; both components are zeroized when dropped.
#[must_use]
pub struct MessageQueueRoute {
    path: String,
    query: Option<String>,
}

impl MessageQueueRoute {
    /// Construct from the path and raw query of a URL whose origin has already
    /// been validated and bound by the host transport. The query is supplied
    /// without a leading `?`.
    ///
    /// # Errors
    ///
    /// Returns `RegistrationRejected` for an unsafe path or malformed raw
    /// query. Origin validation remains the host transport's responsibility.
    pub fn from_parts(mut path: String, mut query: Option<String>) -> Result<Self, WireError> {
        if !valid_queue_path(&path)
            || query
                .as_deref()
                .is_some_and(|value| !valid_queue_query(value))
        {
            path.zeroize();
            if let Some(query) = &mut query {
                query.zeroize();
            }
            return Err(WireError::RegistrationRejected);
        }
        Ok(Self { path, query })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub(crate) fn duplicate(&self) -> Self {
        Self {
            path: self.path.clone(),
            query: self.query.clone(),
        }
    }

    /// Merge the positive cursor using the pinned Go `url.Values.Set` and
    /// `Encode` behavior. Existing query keys and values are retained; all
    /// existing `lastMessageId` values are replaced by the one cursor.
    pub(crate) fn poll_query(&self, cursor: i64) -> Option<String> {
        if cursor <= 0 {
            return self.query.clone();
        }
        let mut pairs = match self.query.as_deref() {
            Some(query) => parse_query_pairs(query)?,
            None => Vec::new(),
        };
        pairs.retain(|(key, _)| key != b"lastMessageId");
        pairs.push((b"lastMessageId".to_vec(), cursor.to_string().into_bytes()));
        (!pairs.is_empty()).then(|| encode_query_pairs(pairs))
    }

    /// Append the exact message id to the URL path, preserving the queue query.
    pub(crate) fn acknowledgement_path(&self, message_id: i64) -> String {
        format!("{}/{message_id}", self.path)
    }

    /// Check that a poll request uses this exact queue path and either the
    /// original query or the pinned positive-cursor rewrite of that query.
    /// This lets a host transport enforce the route without reproducing query
    /// parsing or canonicalization rules.
    #[must_use]
    pub fn matches_poll_target(&self, path: &str, query: Option<&str>) -> bool {
        if path != self.path || query == self.query.as_deref() {
            return path == self.path && query == self.query.as_deref();
        }
        let Some(query) = query else {
            return false;
        };
        let Some(mut pairs) = parse_query_pairs(query) else {
            return false;
        };
        let mut cursor = None;
        for (key, value) in &pairs {
            if key == b"lastMessageId" {
                let Ok(value) = std::str::from_utf8(value) else {
                    cursor = None;
                    break;
                };
                let Ok(value) = value.parse::<i64>() else {
                    cursor = None;
                    break;
                };
                if value <= 0 || cursor.replace(value).is_some() {
                    cursor = None;
                    break;
                }
            }
        }
        zeroize_query_pairs(&mut pairs);
        let Some(cursor) = cursor else {
            return false;
        };
        let mut expected = self.poll_query(cursor);
        let matches = expected.as_deref() == Some(query);
        if let Some(expected) = &mut expected {
            expected.zeroize();
        }
        matches
    }

    /// Check that an ACK request appends one canonical non-negative message
    /// ID to this exact queue path and preserves the original query verbatim.
    #[must_use]
    pub fn matches_ack_target(&self, path: &str, query: Option<&str>) -> bool {
        if query != self.query.as_deref() {
            return false;
        }
        let Some(suffix) = path.strip_prefix(&self.path) else {
            return false;
        };
        let Some(decimal_id) = suffix.strip_prefix('/') else {
            return false;
        };
        let Ok(message_id) = decimal_id.parse::<i64>() else {
            return false;
        };
        message_id >= 0 && message_id.to_string() == decimal_id
    }

    pub(crate) fn same_target(&self, other: &Self) -> bool {
        self.path == other.path && self.query == other.query
    }

    pub(crate) fn replace_with(&mut self, other: Self) {
        self.retire();
        *self = other;
    }

    pub(crate) fn retire(&mut self) {
        self.path.zeroize();
        if let Some(query) = &mut self.query {
            query.zeroize();
        }
        self.query = None;
    }
}

impl fmt::Debug for MessageQueueRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MessageQueueRoute")
            .field("path", &"[redacted]")
            .field("query", &self.query.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl Drop for MessageQueueRoute {
    fn drop(&mut self) {
        self.retire();
    }
}

fn valid_queue_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 4096
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'?' | b'#' | b'\\')
        })
    {
        return false;
    }
    let bytes = path.as_bytes();
    let mut segment = Vec::new();
    let mut index = 1;
    while index <= bytes.len() {
        if index == bytes.len() || bytes[index] == b'/' {
            if segment == b"." || segment == b".." {
                return false;
            }
            segment.clear();
            index += 1;
            continue;
        }
        let byte = if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return false;
            }
            let Some(high) = hex(bytes[index + 1]) else {
                return false;
            };
            let Some(low) = hex(bytes[index + 2]) else {
                return false;
            };
            index += 3;
            (high << 4) | low
        } else {
            let byte = bytes[index];
            index += 1;
            byte
        };
        if byte.is_ascii_control() || matches!(byte, b'/' | b'\\') {
            return false;
        }
        segment.push(byte);
    }
    true
}

fn valid_queue_query(query: &str) -> bool {
    let bytes = query.as_bytes();
    if bytes.len() > 8192
        || bytes.iter().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'#' | b'\\' | b';')
        })
    {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || hex(bytes[index + 1]).is_none()
                || hex(bytes[index + 2]).is_none()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn parse_query_pairs(query: &str) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    if !valid_queue_query(query) {
        return None;
    }
    let mut pairs = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let Some(key) = query_unescape(key) else {
            zeroize_query_pairs(&mut pairs);
            return None;
        };
        let Some(value) = query_unescape(value) else {
            let mut key = key;
            key.zeroize();
            zeroize_query_pairs(&mut pairs);
            return None;
        };
        pairs.push((key, value));
    }
    Some(pairs)
}

fn zeroize_query_pairs(pairs: &mut [(Vec<u8>, Vec<u8>)]) {
    for (key, value) in pairs {
        key.zeroize();
        value.zeroize();
    }
}

fn query_unescape(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                let high = hex(*bytes.get(index + 1)?)?;
                let low = hex(*bytes.get(index + 2)?)?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    Some(decoded)
}

fn encode_query_pairs(mut pairs: Vec<(Vec<u8>, Vec<u8>)>) -> String {
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut encoded = String::new();
    for (index, (mut key, mut value)) in pairs.into_iter().enumerate() {
        if index > 0 {
            encoded.push('&');
        }
        query_escape(&key, &mut encoded);
        encoded.push('=');
        query_escape(&value, &mut encoded);
        key.zeroize();
        value.zeroize();
    }
    encoded
}

fn query_escape(value: &[u8], output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.iter().copied() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else if byte == b' ' {
            output.push('+');
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
