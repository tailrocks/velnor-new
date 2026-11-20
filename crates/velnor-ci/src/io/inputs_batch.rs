// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{cold_on_failure, input_scopes, input_target, inputs, restored_targets, scope_cache};
use crate::error::CiError;
use crate::scope::Scope;

pub(super) fn save_args(root: &Path, args: &crate::cli::InputsSaveArgs) -> Result<String, CiError> {
    let target = if let Some(variable) = &args.target_env {
        let encoded = std::env::var(variable)
            .map_err(|error| CiError::new(format!("cannot read {variable}: {error}")))?;
        decode_path(&encoded)?
    } else {
        args.input.target_dir.clone()
    };
    save(root, &target, args.scope_cache.as_deref())
}

pub(super) fn cache_paths(
    args: &crate::cli_cache::DecideArgs,
) -> Result<(PathBuf, Option<PathBuf>), CiError> {
    if let Some(variable) = &args.paths_env {
        let (target, archive): (String, Option<String>) = read_paths(variable)?;
        Ok((
            decode_path(&target)?,
            archive.as_deref().map(decode_path).transpose()?,
        ))
    } else {
        Ok((
            args.target_dir
                .clone()
                .ok_or_else(|| CiError::new("cache decide needs a target path"))?,
            args.archive.clone(),
        ))
    }
}

pub(super) fn restore_args(
    checkout: &Path,
    args: &crate::cli::InputsBatchArgs,
) -> Result<String, CiError> {
    let pairs = if let Some(variable) = &args.roots_env {
        let roots: Vec<[String; 2]> = read_paths(variable)?;
        roots
            .into_iter()
            .flatten()
            .map(|path| decode_path(&path))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        args.root_target.clone()
    };
    if pairs.is_empty() {
        return Err(CiError::new(
            "inputs restore needs at least one root and target",
        ));
    }
    restore_all(checkout, &pairs, args.scope_cache.as_deref())
}

pub(super) fn restored_targets(args: &crate::cli::RestoredArgs) -> Result<Vec<PathBuf>, CiError> {
    if let Some(variable) = &args.targets_env {
        let encoded: Vec<String> = read_paths(variable)?;
        let targets = encoded
            .iter()
            .map(|path| decode_path(path))
            .collect::<Result<Vec<_>, _>>()?;
        if targets.is_empty() {
            return Err(CiError::new("restored needs at least one target"));
        }
        Ok(targets)
    } else {
        Ok(args.target_dir.clone())
    }
}

fn read_paths<T: serde::de::DeserializeOwned>(variable: &str) -> Result<T, CiError> {
    let text = std::env::var(variable)
        .map_err(|error| CiError::new(format!("cannot read {variable}: {error}")))?;
    serde_json::from_str(&text)
        .map_err(|error| CiError::new(format!("cannot parse {variable}: {error}")))
}

fn decode_path(encoded: &str) -> Result<PathBuf, CiError> {
    let (pairs, extra) = encoded.as_bytes().as_chunks::<2>();
    if !extra.is_empty() {
        return Err(CiError::new(
            "path specification contains an incomplete hex byte",
        ));
    }
    let bytes = pairs
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair)
                .map_err(|error| CiError::new(error.to_string()))
                .and_then(|text| {
                    u8::from_str_radix(text, 16).map_err(|error| CiError::new(error.to_string()))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|error| CiError::new(error.to_string()))
}

pub(super) fn restore_all(
    checkout: &Path,
    pairs: &[PathBuf],
    cache: Option<&Path>,
) -> Result<String, CiError> {
    restore_inputs_batch_with(checkout, pairs, cache, &scope_cache::FileSystem)
}

pub fn restore_inputs_batch_with(
    checkout: &Path,
    pairs: &[PathBuf],
    cache: Option<&Path>,
    io: &dyn scope_cache::ScopeCacheIo,
) -> Result<String, CiError> {
    let roots: Vec<_> = pairs
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            Ok((
                input_target::resolve(&checkout.join(&pair[0]))?,
                checkout.join(&pair[1]),
            ))
        })
        .collect::<Result<_, CiError>>()?;
    for (root, target) in &roots {
        input_target::check_target(root, target)?;
    }
    if let Some(cache) = cache {
        input_target::check_target(checkout, cache)?;
        scope_cache::discard(cache)?;
    }
    let top = inputs::tracked_paths(checkout)?.top;
    let context = input_scopes::context_with(&top, io)?;
    let mut scoped = Vec::new();
    for (root, target) in roots {
        let prefix = root.strip_prefix(&top).map_err(|e| {
            CiError::new(format!("root {} is outside checkout: {e}", root.display()))
        })?;
        let prefix = prefix
            .to_str()
            .ok_or_else(|| CiError::new("root is not valid UTF-8"))?;
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let scope = Scope::new(&prefix, &context.listed, context.facts.as_ref());
        scoped.push((root, target, scope));
    }
    if let Some(cache) = cache {
        let by_root = scoped
            .iter()
            .map(|(root, _, scope)| (root.clone(), scope.clone()))
            .collect::<BTreeMap<_, _>>();
        scope_cache::write(cache, &top, &context, by_root, io)?;
    }
    let mut report = String::new();
    let mut scans = BTreeMap::new();
    for (root, target, scope) in scoped {
        restored_targets::record(&root, &target)?;
        report.push_str(&cold_on_failure(
            std::slice::from_ref(&target),
            "inputs restore",
            || inputs::restore_inputs_scoped(&root, &target, Some(&scope), &mut scans),
        )?);
    }
    Ok(report)
}

pub(super) fn save(root: &Path, target: &Path, cache: Option<&Path>) -> Result<String, CiError> {
    input_target::check_target(root, target)?;
    if let Some(cache) = cache {
        input_target::check_target(root, cache)?;
        inputs::invalidate_snapshot(root, target)?;
        let scope = scope_cache::read(cache, root)?;
        inputs::save_inputs_scoped(root, target, Some(&scope))
    } else {
        inputs::save_inputs(root, target)
    }
}
