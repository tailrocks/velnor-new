// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(in super::super) fn stream_frame(stream_type: u8, payload: &[u8]) -> Vec<u8> {
    let Ok(length) = u32::try_from(payload.len()) else {
        return Vec::new();
    };
    let mut frame = vec![stream_type, 0, 0, 0];
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

pub(super) fn query_value(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let mut output = String::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(hex_value).unwrap_or_default();
            let low = bytes.next().and_then(hex_value).unwrap_or_default();
            output.push(char::from((high << 4) | low));
        } else if byte == b'+' {
            output.push(' ');
        } else {
            output.push(char::from(byte));
        }
    }
    output
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
