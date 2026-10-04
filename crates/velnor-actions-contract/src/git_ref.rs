//! Pure Git syntax for fragments appended to a fixed reference namespace.

/// Whether a fragment has supported Git reference structure.
///
/// This is not a shell or expression sanitizer. Callers own their literal
/// alphabet, length, and branch/tag semantics and pass names as fixed argv data.
/// Unlike a complete ref, a fragment need not contain a slash.
#[must_use]
pub fn is_valid_git_ref_fragment(value: &str) -> bool {
    !value.is_empty()
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("@{")
        && !value.contains(['\\', ':', '~', '^', '?', '*', '['])
        && value.bytes().all(|byte| byte.is_ascii_graphic())
        && value.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .rsplit_once('.')
                    .is_none_or(|(_, suffix)| suffix != "lock")
        })
}

#[cfg(test)]
mod tests {
    use super::is_valid_git_ref_fragment;

    #[test]
    fn fragments_preserve_case_and_caller_owned_punctuation() {
        for fragment in [
            "main",
            "HEAD",
            "-main",
            "a/-b",
            "a./b",
            "a.LOCK",
            "a/b.Lock",
            "v1.2.3+build",
            "a$b",
            "a;b",
            "a'b",
            "@",
        ] {
            assert!(is_valid_git_ref_fragment(fragment), "{fragment:?}");
        }
    }

    #[test]
    fn fragments_reject_git_structural_and_special_forms() {
        for fragment in [
            "", "/a", "a/", "a//b", ".a", "a/.b", "a.", "a.lock", "a.lock/b", "a/b.lock", "a..b",
            "a@{b", "a b", "a\nb", "a\0b", "a\u{7f}b", "máin", "a\\b", "a:b", "a~b", "a^b", "a?b",
            "a*b", "a[b",
        ] {
            assert!(!is_valid_git_ref_fragment(fragment), "{fragment:?}");
        }
    }
}
