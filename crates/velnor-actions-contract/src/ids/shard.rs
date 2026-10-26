// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub fn split_shard_suffix(task_id: &str) -> Option<(&str, u32, u32)> {
    let (base, shard) = task_id.rsplit_once("/shard-")?;
    if base.contains("/shard-") {
        return None;
    }
    let (index, count) = shard.split_once("-of-")?;
    if !is_canonical_numeral(index) || !is_canonical_numeral(count) {
        return None;
    }
    let (index, count) = (index.parse::<u32>().ok()?, count.parse::<u32>().ok()?);
    if index == 0 || count == 0 || index > count {
        return None;
    }
    Some((base, index, count))
}

fn is_canonical_numeral(text: &str) -> bool {
    let mut bytes = text.bytes();
    matches!(bytes.next(), Some(b'1'..=b'9')) && bytes.all(|b| b.is_ascii_digit())
}
