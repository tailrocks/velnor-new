//! Closed Git branch grammar for generated expressions, triggers, and argv.
//!
//! This deliberately supports only ASCII letters, digits, slash, dot,
//! underscore, and hyphen. Git accepts additional punctuation that cannot
//! safely cross every workflow expression and shell boundary we emit.

/// Whether a branch is a literal supported Git branch name.
///
/// Names are never patterns or expressions. Components cannot be empty,
/// start with a dot or hyphen, end with a dot or lowercase `.lock`, or
/// contain `..`.
#[must_use]
pub fn is_valid_branch_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "HEAD"
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        && value.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with(['.', '-'])
                && !part.ends_with('.')
                && part
                    .rsplit_once('.')
                    .is_none_or(|(_, suffix)| suffix != "lock")
        })
}

#[cfg(test)]
mod tests {
    use super::is_valid_branch_name;

    #[test]
    fn branch_authority_accepts_literal_names() {
        for branch in [
            "main",
            "trunk",
            "release/1.2",
            "feature/x_y-z",
            "Main",
            "main.LOCK",
            "a/b.Lock",
        ] {
            assert!(is_valid_branch_name(branch), "{branch:?}");
        }
    }

    #[test]
    fn branch_authority_rejects_expression_yaml_shell_and_git_names() {
        for branch in [
            "",
            "HEAD",
            "-main",
            "/main",
            "main/",
            "a//b",
            ".main",
            "a/.b",
            "main.",
            "main.lock",
            "a/b.lock",
            "a..b",
            "a/../b",
            "a/./b",
            "a/-b",
            "a b",
            "a\nb",
            "a\rb",
            "a\tb",
            "a\0b",
            "a\u{7f}b",
            "máin",
            "a'b",
            "a\"b",
            "a|b",
            "a&b",
            "a;b",
            "a`b",
            "a$b",
            "a(b)",
            "a{b}",
            "a[b]",
            "a*b",
            "a?b",
            "a!b",
            "a:b",
            "a~b",
            "a^b",
            "a\\b",
            "a@b",
            "${{github.ref}}",
            "main'||true||'",
            "main\non: [push]",
        ] {
            assert!(!is_valid_branch_name(branch), "{branch:?}");
        }
        assert!(!is_valid_branch_name(&"a".repeat(256)));
    }
}
