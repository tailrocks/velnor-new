use std::{ffi::OsString, path::Path};

use velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTreeChange;
use velnor_actions_mise::{CheckDeadline, GitRequest};

use crate::{OrchestratorError, internal::internal};

pub(super) fn read_transition_diff(
    root: &Path,
    base_sha: &str,
    head_sha: &str,
    deadline: CheckDeadline,
) -> Result<Vec<HomebrewAdoptionTreeChange>, OrchestratorError> {
    let args = [
        "--raw",
        "-z",
        "--no-abbrev",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        base_sha,
        head_sha,
        "--",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let output = GitRequest::diff(args)
        .run_in_until(root, deadline)
        .map_err(|_| internal("homebrew_adoption_git_diff"))?;
    let output = output
        .require_success("git")
        .map_err(|_| internal("homebrew_adoption_git_diff"))?;
    parse_raw_diff(&output.stdout)
}

pub(super) fn parse_raw_diff(
    bytes: &[u8],
) -> Result<Vec<HomebrewAdoptionTreeChange>, OrchestratorError> {
    if bytes.is_empty() {
        return Err(internal("homebrew_adoption_diff_empty"));
    }
    let records = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if records.last() != Some(&&[][..]) || records.len() % 2 != 1 {
        return Err(internal("homebrew_adoption_diff_framing"));
    }
    let mut changes = Vec::with_capacity((records.len() - 1) / 2);
    for pair in records[..records.len() - 1].chunks(2) {
        let metadata = std::str::from_utf8(pair[0])
            .map_err(|_| internal("homebrew_adoption_diff_metadata"))?;
        let path =
            std::str::from_utf8(pair[1]).map_err(|_| internal("homebrew_adoption_diff_path"))?;
        let mut fields = metadata.split_ascii_whitespace();
        let modes = fields
            .next()
            .and_then(|value| value.strip_prefix(':'))
            .ok_or_else(|| internal("homebrew_adoption_diff_modes"))?;
        let old_mode = parse_mode(modes)?;
        let new_mode = parse_mode(
            fields
                .next()
                .ok_or_else(|| internal("homebrew_adoption_diff_modes"))?,
        )?;
        let old_blob_sha = fields
            .next()
            .filter(|value| full_sha(value))
            .ok_or_else(|| internal("homebrew_adoption_diff_old_blob"))?
            .to_owned();
        let new_blob_sha = fields
            .next()
            .filter(|value| full_sha(value))
            .ok_or_else(|| internal("homebrew_adoption_diff_new_blob"))?
            .to_owned();
        let status = fields
            .next()
            .filter(|value| matches!(*value, "A" | "D" | "M" | "T"))
            .ok_or_else(|| internal("homebrew_adoption_diff_status"))?;
        if fields.next().is_some() || !safe_path(path) {
            return Err(internal("homebrew_adoption_diff_record"));
        }
        let (old_mode, old_blob_sha, new_mode, new_blob_sha) = match status {
            "A" => (None, None, Some(new_mode), Some(new_blob_sha)),
            "D" => (Some(old_mode), Some(old_blob_sha), None, None),
            _ => (
                Some(old_mode),
                Some(old_blob_sha),
                Some(new_mode),
                Some(new_blob_sha),
            ),
        };
        changes.push(HomebrewAdoptionTreeChange {
            path: path.to_owned(),
            old_mode,
            new_mode,
            old_blob_sha,
            new_blob_sha,
            status: status.to_owned(),
        });
    }
    if !changes.windows(2).all(|pair| pair[0].path < pair[1].path) {
        return Err(internal("homebrew_adoption_diff_order"));
    }
    Ok(changes)
}

fn parse_mode(value: &str) -> Result<u32, OrchestratorError> {
    if value.len() != 6 || !value.bytes().all(|byte| (b'0'..=b'7').contains(&byte)) {
        return Err(internal("homebrew_adoption_diff_mode"));
    }
    u32::from_str_radix(value, 8).map_err(|_| internal("homebrew_adoption_diff_mode"))
}

pub(super) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
