//! NUL-delimited git path splitting shared by selection call paths.
//!
//! Every `git diff -z` and `git ls-files -z` consumer splits output here,
//! so path bytes stay exact: no C-quoting, no trimming, empty chunks
//! dropped. Non-UTF-8 fails under one tag so callers broaden explicitly.

/// Explicit broaden tag for non-UTF-8 git paths.
pub const NON_UTF8_PATH: &str = "non_utf8_path";

/// Split NUL-delimited git path bytes into exact path strings.
///
/// Empty chunks drop; every other chunk keeps its exact bytes, including
/// whitespace, newlines, and non-ASCII names. Non-UTF-8 fails with
/// [`NON_UTF8_PATH`] so the caller broadens with an explicit tag.
pub fn split_nul_paths<Paths>(stdout: &[u8]) -> Result<Paths, String>
where
    Paths: FromIterator<String>,
{
    stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|chunk| String::from_utf8(chunk.to_vec()).map_err(|_| NON_UTF8_PATH.to_owned()))
        .collect()
}

/// Split NUL-delimited git path bytes, skipping non-UTF-8 entries.
///
/// Returns the decodable paths plus whether any entry was skipped.
/// Index enumeration uses this (a skipped entry cannot match an ASCII
/// detector name); change-set callers keep fail-closed [`split_nul_paths`]
/// because an undecodable changed path cannot be attributed.
pub fn split_nul_paths_skipping(stdout: &[u8]) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut skipped = false;
    for chunk in stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        match String::from_utf8(chunk.to_vec()) {
            Ok(path) => paths.push(path),
            Err(_) => skipped = true,
        }
    }
    (paths, skipped)
}
#[cfg(test)]
mod tests;
