// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

const RUNNER_INVENTORY: &str = include_str!("runner_family_inventory.txt");
pub const RUNNER_INVENTORY_PATH: &str =
    "crates/velnor-runner/crates/velnor-runner-image-manifest/src/runner_family_inventory.txt";

static RUNNER_INPUTS: OnceLock<Vec<&str>> = OnceLock::new();
const RUNNER_ABSENCES: &str = include_str!("runner_family_absences.txt");
pub const RUNNER_ABSENCES_PATH: &str =
    "crates/velnor-runner/crates/velnor-runner-image-manifest/src/runner_family_absences.txt";
static ABSENCES: OnceLock<Vec<&str>> = OnceLock::new();

fn normalized_source_path(path: &str) -> bool {
    path.is_ascii()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}

pub fn parse_runner_publication_source_absences<'a>(
    raw: &'a str,
    positive: &[&str],
) -> Result<Vec<&'a str>, crate::ImageContractError> {
    let entries = raw.lines().collect::<Vec<_>>();
    let invalid =
        || crate::ImageContractError::CanonicalJson("runner_family_absences_invalid".to_owned());
    if entries.is_empty()
        || raw.contains('\r')
        || !raw.ends_with('\n')
        || !entries.windows(2).all(|pair| pair[0] < pair[1])
    {
        return Err(invalid());
    }
    for path in &entries {
        if !normalized_source_path(path)
            || positive.iter().any(|input| {
                *input == *path
                    || input
                        .strip_prefix(path)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                    || path
                        .strip_prefix(input)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        {
            return Err(invalid());
        }
    }
    Ok(entries)
}

pub fn runner_publication_source_absences()
-> Result<&'static [&'static str], crate::ImageContractError> {
    let positive = runner_publication_source_inputs()?;
    if !positive.contains(&RUNNER_ABSENCES_PATH) {
        return Err(crate::ImageContractError::CanonicalJson(
            "runner_family_absences_unbound".to_owned(),
        ));
    }
    let absences = ABSENCES
        .get_or_init(|| {
            parse_runner_publication_source_absences(RUNNER_ABSENCES, positive).unwrap_or_default()
        })
        .as_slice();
    if absences.is_empty() {
        Err(crate::ImageContractError::CanonicalJson(
            "runner_family_absences_invalid".to_owned(),
        ))
    } else {
        Ok(absences)
    }
}

pub fn runner_publication_source_inputs()
-> Result<&'static [&'static str], crate::ImageContractError> {
    let inputs = RUNNER_INPUTS
        .get_or_init(|| {
            parse_runner_publication_source_inputs(RUNNER_INVENTORY).unwrap_or_default()
        })
        .as_slice();
    if inputs.is_empty() {
        Err(crate::ImageContractError::CanonicalJson(
            "runner_family_inventory_invalid".to_owned(),
        ))
    } else {
        Ok(inputs)
    }
}

pub fn parse_runner_publication_source_inputs(
    raw: &str,
) -> Result<Vec<&str>, crate::ImageContractError> {
    let inputs = raw.lines().collect::<Vec<_>>();
    if inputs.is_empty()
        || !raw.ends_with('\n')
        || raw.contains('\r')
        || !inputs.iter().all(|path| normalized_source_path(path))
        || !inputs.windows(2).all(|pair| pair[0] < pair[1])
    {
        Err(crate::ImageContractError::CanonicalJson(
            "runner_family_inventory_invalid".to_owned(),
        ))
    } else {
        Ok(inputs)
    }
}
