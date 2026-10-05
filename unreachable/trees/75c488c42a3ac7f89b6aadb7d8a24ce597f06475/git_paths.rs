//! NUL-delimited git path splitting shared by selection call paths.
//!
//! Every `git diff -z` and `git ls-files -z` consumer splits output here,
//! so path bytes stay exact: no C-quoting, no trimming, empty chunks
//! dropped. Non-UTF-8 fails under one tag so callers broaden explicitly.

/// Explicit broaden tag for non-UTF-8 git paths.
pub(crate) const NON_UTF8_PATH: &str = "non_utf8_path";

/// Split NUL-delimited git path bytes into exact path strings.
///
/// Empty chunks drop; every other chunk keeps its exact bytes, including
/// whitespace, newlines, and non-ASCII names. Non-UTF-8 fails with
/// [`NON_UTF8_PATH`] so the caller broadens with an explicit tag.
pub(crate) fn split_nul_paths<Paths>(stdout: &[u8]) -> Result<Paths, String>
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
pub(crate) fn split_nul_paths_skipping(stdout: &[u8]) -> (Vec<String>, bool) {
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
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Unicode, trailing spaces, and newlines survive verbatim.
    #[test]
    fn exact_bytes_survive() {
        let paths: BTreeSet<String> =
            split_nul_paths("alpha/src/héllo.rs\0alpha/src/trailing.rs \0a/b\nc.rs\0".as_bytes())
                .unwrap_or_default();
        assert!(paths.contains("alpha/src/héllo.rs"), "unicode: {paths:?}");
        assert!(
            paths.contains("alpha/src/trailing.rs "),
            "trailing space kept: {paths:?}"
        );
        assert!(paths.contains("a/b\nc.rs"), "newline kept: {paths:?}");
    }

    /// Leading, trailing, and doubled NULs contribute nothing.
    #[test]
    fn empty_chunks_drop() {
        let paths: BTreeSet<String> =
            split_nul_paths(b"\0alpha/Cargo.toml\0\0beta/Cargo.toml\0").unwrap_or_default();
        assert_eq!(
            paths,
            BTreeSet::from(["alpha/Cargo.toml".to_owned(), "beta/Cargo.toml".to_owned()]),
            "only real chunks: {paths:?}"
        );
    }

    /// Non-UTF-8 fails under the shared explicit tag.
    #[test]
    fn non_utf8_fails_with_tag() {
        let result: Result<BTreeSet<String>, String> =
            split_nul_paths(b"alpha/src/\xffinvalid.rs\0");
        assert_eq!(result, Err(NON_UTF8_PATH.to_owned()));
    }

    /// The splitter collects into ordered lists as well as sets.
    #[test]
    fn collects_into_vec() {
        let paths: Vec<String> = split_nul_paths(b"b\0a\0").unwrap_or_default();
        assert_eq!(paths, vec!["b".to_owned(), "a".to_owned()]);
    }

    /// The skipping splitter keeps decodable entries and flags the rest.
    #[test]
    fn skipping_keeps_decodable_and_flags() {
        let (paths, skipped) =
            split_nul_paths_skipping(b"alpha/Cargo.toml\0alpha/src/\xffinvalid.rs\0");
        assert_eq!(paths, vec!["alpha/Cargo.toml".to_owned()]);
        assert!(skipped, "skip is explicit");
        let (clean, skipped) = split_nul_paths_skipping(b"a\0b\0");
        assert_eq!(clean, vec!["a".to_owned(), "b".to_owned()]);
        assert!(!skipped, "no false flag");
    }
}
