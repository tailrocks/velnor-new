//! Paths taken from `actions/scaleset` `e6daac7`. Not the classic REST JIT route.

/// Collection path relative to the Actions service.
pub(crate) const SCALE_SET_ENDPOINT: &str = "_apis/runtime/runnerscalesets";
/// Collection path for runners in the pinned distributed-task API.
pub(crate) const RUNNER_ENDPOINT: &str = "_apis/distributedtask/pools/0/agents";

/// `GET`/`POST` collection path.
#[must_use]
pub fn scale_set_path() -> &'static str {
    SCALE_SET_ENDPOINT
}

/// `POST .../{id}/acquirejobs`.
#[must_use]
pub fn acquire_path(scale_set_id: i64) -> String {
    format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/acquirejobs")
}

/// `POST .../{id}/generatejitconfig`. Not `generate-jitconfig`.
#[must_use]
pub fn jit_path(scale_set_id: i64) -> String {
    format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/generatejitconfig")
}

/// `GET` runner collection and `DELETE` runner item paths.
pub(crate) fn runner_path(runner_id: i64) -> String {
    format!("{RUNNER_ENDPOINT}/{runner_id}")
}

/// Encode a query value using Go `url.Values.Encode` rules.
pub(crate) fn query_escape(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else if byte == b' ' {
            encoded.push('+');
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'A',
        11 => 'B',
        12 => 'C',
        13 => 'D',
        14 => 'E',
        _ => 'F',
    }
}

/// Total-capacity header. The value is `max_jobs`, not free slots.
pub const CAPACITY_HEADER: &str = "X-ScaleSetMaxCapacity";

/// Query `lastMessageId` only when the id is positive. Zero is a real id but
/// the pinned client omits the query until a message has been observed.
#[must_use]
pub fn last_message_query(last_message_id: i64) -> Option<String> {
    if last_message_id > 0 {
        Some(format!("lastMessageId={last_message_id}"))
    } else {
        None
    }
}

/// Header value for total capacity.
#[must_use]
pub fn capacity_header_value(total: u32) -> String {
    total.to_string()
}
