//! Exact catalog selector validation for canonical tool cache identity.
/// True for `<tool>@<version>` specs (backend paths allowed).
pub(super) fn is_tool_spec(value: &str) -> bool {
    if let Some(version) = [
        "rust[profile=minimal,components=clippy,rustfmt]@",
        "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin,mr_boxington=true]@",
        "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin]@",
        "pipx:reuse[extras=charset-normalizer,uvx_args=\"--python 3.14.7 --no-python-downloads\"]@",
    ]
    .iter()
    .find_map(|prefix| value.strip_prefix(prefix))
    {
        return version.split('.').count() == 3
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    }
    let Some((tool, version)) = value.split_once('@') else {
        return false;
    };
    !tool.is_empty()
        && !version.is_empty()
        && !value.contains(' ')
        && !value.contains('\n')
        && tool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'-' | b'_' | b'.'))
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::is_tool_spec;

    #[test]
    fn desktop_rust_profiles_accept_current_catalog_version() {
        for prefix in [
            "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin]@",
            "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin,mr_boxington=true]@",
        ] {
            assert!(is_tool_spec(&format!("{prefix}1.98.1")), "{prefix}");
        }
    }

    #[test]
    fn desktop_rust_profiles_reject_bad_versions_and_profiles() {
        let prefix =
            "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin]@";
        for version in ["1.98", "1.98.1.0", "1.98.x"] {
            assert!(!is_tool_spec(&format!("{prefix}{version}")), "{version}");
        }
        for profile in [
            "rust[profile=default,components=clippy,rustfmt,targets=aarch64-apple-darwin]@1.98.1",
            "rust[profile=minimal,components=clippy,targets=aarch64-apple-darwin]@1.98.1",
            "rust[profile=minimal,components=clippy,rustfmt,targets=aarch64-apple-darwin,mr_boxington=false]@1.98.1",
        ] {
            assert!(!is_tool_spec(profile), "{profile}");
        }
    }
}
