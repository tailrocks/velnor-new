// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CustomHarness, RepoView, RustCiConfig, RustMember, RustPackage, Target, TargetKind,
};

use crate::error::{DetectError, Reason};
use crate::sibling_features::requested_by_siblings;
use crate::toml_file::table;

const SECTIONS: [(&str, TargetKind); 4] = [
    ("bin", TargetKind::Bin),
    ("example", TargetKind::Example),
    ("bench", TargetKind::Bench),
    ("test", TargetKind::Test),
];

pub(crate) fn custom_harnesses(
    view: &RepoView,
    package: &RustPackage,
) -> Result<Vec<CustomHarness>, DetectError> {
    if !view.tracked_paths.contains(&package.manifest) {
        return Err(DetectError::invalid(
            &package.manifest,
            Reason::UntrackedManifest,
        ));
    }
    let Some(bytes) = view.files.get(&package.manifest) else {
        return Ok(Vec::new());
    };
    let manifest =
        table(bytes).map_err(|reason| DetectError::invalid(&package.manifest, reason))?;
    let defaults = enabled_features(&manifest, &requested_by_siblings(view, package));
    let library = manifest
        .get("lib")
        .into_iter()
        .map(|entry| (entry, TargetKind::Lib));
    let declared = SECTIONS
        .into_iter()
        .flat_map(|(key, kind)| {
            manifest
                .get(key)
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .map(move |entry| (entry, kind))
        })
        .chain(library);
    let mut found = Vec::new();
    for (entry, kind) in declared {
        if entry.get("harness").and_then(toml::Value::as_bool) != Some(false)
            || !gated_on(entry, &defaults)
        {
            continue;
        }
        let name = entry.get("name").and_then(toml::Value::as_str).map_or_else(
            || package.name.as_str().replace('-', "_"),
            ToOwned::to_owned,
        );
        let tested = package
            .targets
            .iter()
            .find(|target| target.name.as_str() == name && target.tests && has_kind(target, kind));
        if let Some(target) = tested {
            let harness = CustomHarness {
                name: target.name.clone(),
                kind,
                required_features: entry
                    .get("required-features")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect(),
            };
            if !found.contains(&harness) {
                found.push(harness);
            }
        }
    }
    Ok(found)
}

pub(crate) fn runs_in_direct_step(
    config: &RustCiConfig,
    member: &RustMember,
    harness: &CustomHarness,
) -> bool {
    !config
        .nextest_harnesses
        .contains(&(member.name.clone(), harness.name.clone()))
}

pub(crate) fn uses_nextest(config: &RustCiConfig, member: &RustMember) -> bool {
    member.targets.iter().any(|target| {
        target.tests
            && !member.custom_harnesses.iter().any(|harness| {
                harness.name == target.name
                    && has_kind(target, harness.kind)
                    && runs_in_direct_step(config, member, harness)
            })
    })
}

pub(crate) fn has_kind(target: &Target, kind: TargetKind) -> bool {
    target.kinds.iter().any(|candidate| {
        *candidate == kind || (kind == TargetKind::Lib && *candidate == TargetKind::ProcMacro)
    })
}

pub(crate) fn selector_id(harness: &CustomHarness) -> String {
    match harness.kind {
        TargetKind::Lib | TargetKind::ProcMacro => "lib".to_owned(),
        kind => format!("{kind:?}:{}", harness.name),
    }
}

fn gated_on(entry: &toml::Value, defaults: &BTreeSet<String>) -> bool {
    entry
        .get("required-features")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .all(|feature| {
            feature
                .as_str()
                .is_some_and(|name| name.contains('/') || defaults.contains(name))
        })
}

pub(crate) fn enabled_features(manifest: &toml::Table, requested: &[String]) -> BTreeSet<String> {
    let features = manifest.get("features").and_then(toml::Value::as_table);
    let implicit = implicit_features(manifest);
    let mut enabled = BTreeSet::new();
    let mut pending = vec!["default".to_owned()];
    pending.extend(requested.iter().cloned());
    while let Some(name) = pending.pop() {
        if !enabled.insert(name.clone()) {
            continue;
        }
        let members = features
            .and_then(|table| table.get(&name))
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_str);
        for member in members {
            match member.split_once('/') {
                _ if member.starts_with("dep:") => {}
                Some((prefix, _)) if prefix.ends_with('?') => {}
                Some((prefix, _)) if implicit.contains(prefix) => pending.push(prefix.to_owned()),
                Some(_) => {}
                None => pending.push(member.to_owned()),
            }
        }
    }
    enabled
}

fn implicit_features(manifest: &toml::Table) -> BTreeSet<String> {
    const TABLES: &[&str] = &["dependencies", "build-dependencies"];
    let targets = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::values)
        .filter_map(toml::Value::as_table);
    let optional: BTreeSet<String> = std::iter::once(manifest)
        .chain(targets)
        .flat_map(|scope| TABLES.iter().filter_map(|key| scope.get(*key)))
        .filter_map(toml::Value::as_table)
        .flat_map(toml::Table::iter)
        .filter(|(_, spec)| spec.get("optional").and_then(toml::Value::as_bool) == Some(true))
        .map(|(name, _)| name.clone())
        .collect();
    let explicit: BTreeSet<&str> = manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::values)
        .filter_map(toml::Value::as_array)
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter_map(|member| member.strip_prefix("dep:"))
        .collect();
    optional
        .into_iter()
        .filter(|name| !explicit.contains(name.as_str()))
        .collect()
}
