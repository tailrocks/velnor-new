//! Canonical shard-suffix parsing for task IDs.
//!
//! Every shard question in the workspace routes through
//! [`split_shard_suffix`]: writers, lanes, base strips, and validation.
//! Ad-hoc `split_once`/`rsplit_once` copies disagreed on doubled and
//! mid-ID suffixes; this parser settles them by construction.

/// Split a trailing `/shard-<index>-of-<count>` suffix.
///
/// Returns the unsharded base plus index/count only when the suffix is
/// the final segment, `1 <= index <= count`, and the base carries no
/// shard segment of its own. Doubled or mid-ID suffixes never parse,
/// so every caller agrees on shardedness.
#[must_use]
pub fn split_shard_suffix(task_id: &str) -> Option<(&str, u32, u32)> {
    let (base, shard) = task_id.rsplit_once("/shard-")?;
    if base.contains("/shard-") {
        return None;
    }
    let (index, count) = shard.split_once("-of-")?;
    let (index, count) = (index.parse::<u32>().ok()?, count.parse::<u32>().ok()?);
    if index == 0 || count == 0 || index > count {
        return None;
    }
    Some((base, index, count))
}
