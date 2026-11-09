// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Entry, ImagePublicationProfile};
use crate::{OrchestratorError, internal::internal};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    RUNNER_ABSENCES_PATH, RUNNER_INVENTORY_PATH, parse_runner_publication_source_absences,
    parse_runner_publication_source_inputs, runner_publication_source_absences,
};

pub(super) fn verify(
    profile: ImagePublicationProfile,
    files: &BTreeMap<String, Vec<u8>>,
    paths: &[&str],
    entries: &[Entry],
) -> Result<(), OrchestratorError> {
    match profile {
        ImagePublicationProfile::Job => return Ok(()),
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => {}
    }
    let positive = parse_runner_publication_source_inputs(text(files, RUNNER_INVENTORY_PATH)?)
        .map_err(|_| internal("image_publication_source_inventory_binding"))?;
    if positive.as_slice() != paths {
        return Err(internal("image_publication_source_inventory_binding"));
    }
    let absent =
        parse_runner_publication_source_absences(text(files, RUNNER_ABSENCES_PATH)?, &positive)
            .map_err(|_| internal("image_publication_source_inventory_binding"))?;
    let expected = runner_publication_source_absences()
        .map_err(|_| internal("image_publication_source_inventory_binding"))?;
    if absent.as_slice() != expected {
        return Err(internal("image_publication_source_inventory_binding"));
    }
    verify_absence(entries, &absent)
}

fn text<'a>(
    files: &'a BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<&'a str, OrchestratorError> {
    let bytes = files
        .get(path)
        .ok_or_else(|| internal("image_publication_source_inventory_binding"))?;
    std::str::from_utf8(bytes).map_err(|_| internal("image_publication_source_inventory_binding"))
}

fn verify_absence(entries: &[Entry], absent: &[&str]) -> Result<(), OrchestratorError> {
    let mut by_path = BTreeMap::new();
    for entry in entries {
        if by_path.insert(entry.path.as_str(), entry).is_some() {
            return Err(internal("image_publication_source_tree"));
        }
    }
    for path in absent {
        if by_path
            .keys()
            .any(|candidate| *candidate == *path || descendant(candidate, path))
        {
            return Err(internal("image_publication_source_absence"));
        }
        for (at, _) in path.match_indices('/') {
            let parent = &path[..at];
            match by_path.get(parent) {
                Some(entry)
                    if entry.kind == "tree"
                        && entry.mode == "040000"
                        && super::super::is_sha1(&entry.sha) => {}
                Some(_) => return Err(internal("image_publication_source_absence")),
                None if by_path
                    .keys()
                    .any(|candidate| descendant(candidate, parent)) =>
                {
                    return Err(internal("image_publication_source_absence"));
                }
                None => {}
            }
        }
    }
    Ok(())
}

fn descendant(path: &str, parent: &str) -> bool {
    path.strip_prefix(parent)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
