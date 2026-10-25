use crate::ContractError;
use std::ffi::OsStr;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct FormulaSlots {
    pub(super) source: usize,
    pub(super) version: usize,
    pub(super) urls: usize,
    pub(super) hashes: usize,
}

pub(super) fn parse_source_sha(line: &str) -> Option<&str> {
    line.strip_prefix("# source-sha: ")?.strip_suffix('\n')
}

pub(super) fn parse_version(line: &str) -> Option<&str> {
    line.strip_prefix("  version \"")?.strip_suffix("\"\n")
}

pub(super) fn parse_url(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("      url \"")
        .or_else(|| line.strip_prefix("    url \""))?;
    rest.strip_suffix("\"\n")
}

pub(super) fn parse_sha256(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("      sha256 \"")
        .or_else(|| line.strip_prefix("    sha256 \""))?;
    rest.strip_suffix("\"\n")
}

pub(super) fn parse_metadata_prefix(line: &str) -> bool {
    [
        "# source-sha:",
        "  version ",
        "      url ",
        "      sha256 ",
        "    url ",
        "    sha256 ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

pub(super) fn parse_release_url<'a>(
    value: &'a str,
    repository: &str,
) -> Result<(&'a str, &'a str), ContractError> {
    let prefix = format!("https://github.com/{repository}/releases/download/");
    let rest = value
        .strip_prefix(&prefix)
        .ok_or_else(|| invalid("formula_url_repository_mismatch"))?;
    let (tag, name) = rest
        .split_once('/')
        .ok_or_else(|| invalid("formula_url_shape"))?;
    if tag.is_empty() || name.is_empty() || name.contains('/') {
        return Err(invalid("formula_url_shape"));
    }
    Ok((tag, name))
}

pub(super) fn spaces(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches(' ').len()]
}

pub(super) fn safe_formula_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && std::path::Path::new(value).extension() == Some(OsStr::new("rb"))
}

pub(super) fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(is_lower_hex)
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_hex)
}

pub(super) fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn invalid(reason: &'static str) -> ContractError {
    ContractError::identity("homebrew_formula", reason)
}
