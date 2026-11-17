// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{
    cell::RefCell,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[path = "homebrew_qualification_evidence/io.rs"]
mod io;
use io::{read_file, require_private_directory};

thread_local! {
    static ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ROOT.with(|root| *root.borrow_mut() = None);
    }
}

pub(crate) fn active() -> bool {
    ROOT.with(|root| root.borrow().is_some())
}

pub(crate) fn scope<T>(
    root: &Path,
    operation: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    if active() || !root.is_absolute() {
        return Err(internal("homebrew_evidence_scope"));
    }
    require_private_directory(root)?;
    ROOT.with(|slot| *slot.borrow_mut() = Some(root.to_owned()));
    let _reset = Reset;
    operation()
}

pub(crate) fn read(
    accept: &str,
    route: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, OrchestratorError> {
    ROOT.with(|root| {
        let root = root.borrow();
        let Some(root) = root.as_ref() else {
            return Ok(None);
        };
        let name = crate::generator::sha256_hex(format!("{accept}\n{route}").as_bytes());
        read_file(&root.join(name), limit as u64).map(Some)
    })
}

pub(crate) fn request(
    args: &[OsString],
    accept: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, OrchestratorError> {
    if !active() {
        return Ok(None);
    }
    let route = args
        .first()
        .and_then(|route| route.to_str())
        .ok_or_else(|| internal("homebrew_evidence_request"))?;
    let route = route.split_once("&_velnor_read_nonce=").map_or_else(
        || {
            route
                .split_once("?_velnor_read_nonce=")
                .map_or(route, |(route, _)| route)
        },
        |(route, _)| route,
    );
    if !route.starts_with("repos/jackin-project/jackin/") && route != "repos/jackin-project/jackin"
    {
        return Err(internal("homebrew_evidence_repository"));
    }
    if args.len() != 1
        && (args.len() != 5
            || args[1] != "-H"
            || args[2] != "Cache-Control: no-cache, no-store"
            || args[3] != "-H"
            || args[4] != "Pragma: no-cache")
    {
        return Err(internal("homebrew_evidence_read_only"));
    }
    read(accept, route, limit)
}

pub(crate) fn trusted_root() -> Result<Option<PathBuf>, OrchestratorError> {
    ROOT.with(|root| {
        let root = root.borrow();
        let Some(root) = root.as_ref() else {
            return Ok(None);
        };
        let path = root.join("trusted_root.jsonl");
        read_file(&path, 4 * 1024 * 1024)?;
        Ok(Some(path))
    })
}
