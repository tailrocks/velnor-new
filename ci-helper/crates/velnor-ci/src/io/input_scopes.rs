// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use velnor_model::{CompileInputs, RepoFacts, RepoPath};
use velnor_rust::{INPUTS_LOCK, METADATA_PATH, detect, parse_inputs_lock};

use super::inputs::{git, top_level_files, tracked_paths};
use crate::error::CiError;
use crate::inputs::Tracked;
use crate::scope::Scope;

pub(super) struct Context {
    pub index: String,
    pub listed: CompileInputs,
    pub facts: Option<RepoFacts>,
    pub files: BTreeMap<RepoPath, Vec<u8>>,
}

pub(super) fn context(top: &Path) -> Result<Context, CiError> {
    context_with(top, &super::scope_cache::FileSystem)
}

pub(super) fn context_with(
    top: &Path,
    io: &dyn super::scope_cache::ScopeCacheIo,
) -> Result<Context, CiError> {
    let index = super::scope_cache::index(top)?;
    let mut files = BTreeMap::new();
    let mut initial = BTreeMap::new();
    for path in [INPUTS_LOCK, METADATA_PATH] {
        let bytes = io.read_file(&top.join(path.as_str()))?;
        if let Some(bytes) = &bytes {
            files.insert(path.clone(), bytes.clone());
        }
        initial.insert(path, bytes);
    }
    let listed = files.get(&INPUTS_LOCK).map_or_else(
        || Ok(CompileInputs::new()),
        |bytes| parse_inputs_lock(bytes).map_err(|e| CiError::new(e.to_string())),
    )?;
    let facts = if files.contains_key(&METADATA_PATH) {
        let checkout = super::report_checkout::read(top)?.checkout;
        for (path, bytes) in &checkout.view.files {
            if initial
                .get(path)
                .is_some_and(|before| before.as_ref() != Some(bytes))
                || files.get(path).is_some_and(|before| before != bytes)
            {
                return Err(CiError::new(format!(
                    "input {path} changed while detecting scopes"
                )));
            }
            files.insert(path.clone(), bytes.clone());
        }
        Some(detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?)
    } else {
        None
    };
    Ok(Context {
        index,
        listed,
        facts,
        files,
    })
}

pub(super) fn git_prefix(root: &Path) -> Result<String, CiError> {
    let prefix = git(root, &["rev-parse", "--show-prefix"], None)?;
    let prefix = String::from_utf8(prefix)
        .map_err(|e| CiError::new(format!("git rev-parse printed non-UTF-8: {e}")))?;
    Ok(prefix.trim_end_matches(['\n', '\r']).to_owned())
}

pub(super) fn scoped_files(
    root: &Path,
    scope: Option<&Scope>,
) -> Result<(PathBuf, Tracked), CiError> {
    let top = tracked_paths(root)?.top;
    let owned;
    let scope = if let Some(scope) = scope {
        scope
    } else {
        let context = context(&top)?;
        owned = Scope::new(&git_prefix(root)?, &context.listed, context.facts.as_ref());
        &owned
    };
    Ok((top.clone(), top_level_files(&top, scope)?))
}
