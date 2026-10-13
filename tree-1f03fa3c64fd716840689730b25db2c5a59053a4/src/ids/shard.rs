//! Canonical shard-suffix parsing for task IDs.
//!
//! Every shard question in the workspace routes through
//! [`split_shard_suffix`]: writers, lanes, base strips, and validation.
//! Ad-hoc `split_once`/`rsplit_once` copies disagreed on doubled and
//! mid-ID suffixes; this parser settles them by construction.

/// Split a trailing `/shard-<index>-of-<count>` suffix.
///
/// Returns the unsharded base plus index/count only when the suffix is
/// the final segment, `1 <= index <= count`, both numerals are
/// canonical (`1-9` first byte, ASCII digits after), and the base
/// carries no shard segment of its own. Doubled, mid-ID, or
/// non-canonical (`+1`, `01`) suffixes never parse, so validated IDs
/// are always canonical and no alias collides with a canonical shard.
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

/// True for canonical shard numerals: `1-9` first, ASCII digits after.
///
/// `u32::from_str` alone accepts `+1` and `01`, letting
/// `shard-+1-of-02` alias `shard-1-of-2`; the round-trip shape check
/// keeps validated identical to canonical.
fn is_canonical_numeral(text: &str) -> bool {
    let mut bytes = text.bytes();
    matches!(bytes.next(), Some(b'1'..=b'9')) && bytes.all(|b| b.is_ascii_digit())
}
