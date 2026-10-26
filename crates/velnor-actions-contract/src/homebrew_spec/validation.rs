// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ContractError, PackageReleaseChannel};

pub(super) fn safe_formula_name(value: &str) -> bool {
    safe_identifier(value) && value.len() <= 64
}
pub(super) fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && value.as_bytes()[0].is_ascii_lowercase()
}
pub(super) fn safe_binary_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
pub(super) fn safe_target(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}
pub(super) fn supported_formula_target(value: &str) -> bool {
    safe_target(value)
        && matches!(
            value,
            "aarch64-apple-darwin"
                | "x86_64-apple-darwin"
                | "aarch64-unknown-linux-gnu"
                | "x86_64-unknown-linux-gnu"
        )
}
pub(super) fn safe_formula_path(value: &str) -> bool {
    value.starts_with("Formula/")
        && safe_relative_path(value)
        && std::path::Path::new(value).extension() == Some(std::ffi::OsStr::new("rb"))
}
pub(super) fn safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with('/')
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}
pub(super) fn safe_archive_member(value: &str) -> bool {
    safe_relative_path(value) && !value.contains('/')
}
pub(super) fn valid_ruby_string(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
pub(super) fn valid_https_url(value: &str) -> bool {
    value.starts_with("https://")
        && !value.contains(['\n', '\r', '\0', '\"', '\\'])
        && value
            .split('/')
            .nth(2)
            .is_some_and(|host| !host.is_empty() && !host.contains('@'))
}
pub(super) fn valid_spdx_expression(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b' ' | b'(' | b')'))
}
pub(super) fn valid_iso_date(value: &str) -> bool {
    if !(value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()))
    {
        return false;
    }
    let month = value[5..7].parse::<u8>().ok();
    let day = value[8..10].parse::<u8>().ok();
    let year = value[..4].parse::<u16>().ok();
    match (year, month, day) {
        (Some(year), Some(month @ 1..=12), Some(day)) => {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let days = match month {
                2 if leap => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            day >= 1 && day <= days
        }
        _ => false,
    }
}
pub(super) fn sorted_unique_strings(values: &[String], valid: fn(&str) -> bool) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1]) && values.iter().all(|value| valid(value))
}
pub(super) fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(super) fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}
pub(super) fn safe_release_tag(
    value: &str,
    channel: PackageReleaseChannel,
    source: &str,
    version: &str,
) -> bool {
    match channel {
        PackageReleaseChannel::Preview => value == format!("preview-{source}"),
        PackageReleaseChannel::Stable => value == format!("v{version}"),
    }
}
pub(super) fn release_version_matches_plan(
    channel: PackageReleaseChannel,
    base_version: &str,
    source_sha: &str,
    version: &str,
) -> bool {
    match channel {
        PackageReleaseChannel::Stable => version == base_version,
        PackageReleaseChannel::Preview => {
            version
                .split_once("-preview.")
                .is_some_and(|(version_base, _)| version_base == base_version)
                && canonical_channel_version_matches(channel, source_sha, version)
        }
    }
}
pub(super) fn canonical_channel_version_matches(
    channel: PackageReleaseChannel,
    source_sha: &str,
    version: &str,
) -> bool {
    match channel {
        PackageReleaseChannel::Stable => crate::config::is_exact_semver(version),
        PackageReleaseChannel::Preview => {
            let Some((base, suffix)) = version.split_once("-preview.") else {
                return false;
            };
            let Some((count, short_sha)) = suffix.split_once('+') else {
                return false;
            };
            let Ok(commit_count) = count.parse::<u64>() else {
                return false;
            };
            crate::config::is_exact_semver(base)
                && commit_count > 0
                && commit_count.to_string() == count
                && short_sha.len() == 7
                && short_sha == &source_sha[..7]
        }
    }
}
pub(super) fn ruby_class_name(value: &str) -> String {
    value
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}
pub(super) fn ruby_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("#{", "\\#{")
        .replace("#@", "\\#@")
        .replace("#$", "\\#$");
    format!("\"{escaped}\"")
}
pub(super) fn ruby_assignment(out: &mut String, key: &str, value: &str) {
    out.push_str("  ");
    out.push_str(key);
    out.push(' ');
    out.push_str(&ruby_quote(value));
    out.push('\n');
}
pub(super) fn ruby_platform_open(target: &str) -> Result<&'static str, ContractError> {
    match target {
        "aarch64-apple-darwin" => Ok("  on_macos do\n    on_arm do\n"),
        "x86_64-apple-darwin" => Ok("  on_macos do\n    on_intel do\n"),
        "aarch64-unknown-linux-gnu" => Ok("  on_linux do\n    on_arm do\n"),
        "x86_64-unknown-linux-gnu" => Ok("  on_linux do\n    on_intel do\n"),
        _ => Err(invalid("unsupported_formula_target")),
    }
}
pub(super) fn ruby_cpu_predicate(target: &str) -> Result<&'static str, ContractError> {
    match formula_cpu_class(target) {
        Some("arm") => Ok("Hardware::CPU.arm?"),
        Some("intel") => Ok("Hardware::CPU.intel?"),
        _ => Err(invalid("unsupported_formula_target")),
    }
}
pub(super) fn formula_cpu_class(target: &str) -> Option<&'static str> {
    match target {
        "aarch64-unknown-linux-gnu" | "aarch64-apple-darwin" => Some("arm"),
        "x86_64-unknown-linux-gnu" | "x86_64-apple-darwin" => Some("intel"),
        _ => None,
    }
}
pub(super) fn unquote(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}
pub(super) fn channel_key(channel: PackageReleaseChannel) -> &'static str {
    match channel {
        PackageReleaseChannel::Preview => "preview",
        PackageReleaseChannel::Stable => "stable",
    }
}
pub(super) fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("homebrew_formula", problem)
}
