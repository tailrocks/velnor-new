// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::gc1_scene::fail;

pub(super) fn decode_supervisor_hex(payload: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if payload.len() > 49_152
        || payload.is_empty()
        || payload.len() % 2 != 0
        || !payload
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(fail(
            "selector supervisor environment is not bounded lowercase hex",
        ));
    }
    let (pairs, remainder) = payload.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(fail("selector supervisor payload has an incomplete byte"));
    }
    pairs
        .iter()
        .map(|pair| {
            let high = char::from(pair[0])
                .to_digit(16)
                .ok_or_else(|| fail("bad supervisor hex"))?;
            let low = char::from(pair[1])
                .to_digit(16)
                .ok_or_else(|| fail("bad supervisor hex"))?;
            u8::try_from(high * 16 + low).map_err(|_| fail("supervisor byte is out of range"))
        })
        .collect()
}
