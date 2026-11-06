// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::get_json;
use crate::{OrchestratorError, github_authority_read, internal::internal};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha1_checked::{Digest, Sha1};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    io::{Cursor, Read},
    path::{Component, Path},
};
use velnor_actions_contract::{IMAGE_NATIVE_INVENTORY_PATH, NativeImageSourceInventory};
use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};

const HEX: &[u8; 16] = b"0123456789abcdef";
const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SELECTED_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Deserialize)]
struct Commit {
    sha: String,
    tree: Object,
}
#[derive(Deserialize)]
struct Object {
    sha: String,
}
#[derive(Deserialize)]
struct Tree {
    sha: String,
    truncated: bool,
    #[serde(rename = "tree")]
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

pub(super) fn source_files(
    source: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let tree = load_tree(source, token, deadline)?;
    let inventory_ids = select_tree_paths(&tree, &[IMAGE_NATIVE_INVENTORY_PATH])?;
    let bytes = download(source, token, deadline)?;
    let inventory_files = read_archive(&bytes, &inventory_ids, source, deadline)?;
    let inventory_bytes = inventory_files
        .get(IMAGE_NATIVE_INVENTORY_PATH)
        .ok_or_else(|| internal("image_publication_source_inventory"))?;
    let inventory = NativeImageSourceInventory::parse(inventory_bytes)
        .map_err(|_| internal("image_publication_source_inventory"))?;
    let paths = inventory.paths().collect::<Vec<_>>();
    let ids = select_tree_paths(&tree, &paths)?;
    read_archive(&bytes, &ids, source, deadline)
}

fn load_tree(
    source: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Tree, OrchestratorError> {
    let commit: Commit = get_json(
        &format!("repos/tailrocks/velnor-new/git/commits/{source}"),
        token,
        deadline,
    )?;
    if commit.sha != source || !super::is_sha1(&commit.tree.sha) {
        return Err(internal("image_publication_source_commit"));
    }
    let tree: Tree = get_json(
        &format!(
            "repos/tailrocks/velnor-new/git/trees/{}?recursive=1",
            commit.tree.sha
        ),
        token,
        deadline,
    )?;
    if tree.sha != commit.tree.sha || tree.truncated || tree.entries.len() > 100_000 {
        return Err(internal("image_publication_source_tree"));
    }
    Ok(tree)
}

fn select_tree_paths(
    tree: &Tree,
    paths: &[&str],
) -> Result<BTreeMap<String, (String, String)>, OrchestratorError> {
    let wanted = paths.iter().copied().collect::<BTreeSet<_>>();
    if wanted.len() != paths.len() {
        return Err(internal("image_publication_source_inventory"));
    }
    let mut ids = BTreeMap::new();
    for entry in &tree.entries {
        if wanted.contains(entry.path.as_str())
            && (entry.kind != "blob"
                || !matches!(entry.mode.as_str(), "100644" | "100755")
                || !super::is_sha1(&entry.sha)
                || ids
                    .insert(entry.path.clone(), (entry.sha.clone(), entry.mode.clone()))
                    .is_some())
        {
            return Err(internal("image_publication_source_tree"));
        }
    }
    if ids.len() != wanted.len() {
        return Err(internal("image_publication_source_inventory"));
    }
    Ok(ids)
}

fn download(
    source: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let route = format!("repos/tailrocks/velnor-new/tarball/{source}");
    let mut args = vec![OsString::from("api")];
    args.extend(github_authority_read::arguments(&route)?);
    let command = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("image_publication_gh_unavailable"))?
        .command_with_github_token(&ToolCatalog::pinned(), token)
        .map_err(|_| internal("image_publication_gh_token"))?;
    let (output, _) = command
        .run_quiescent_until(MAX_ARCHIVE_BYTES, deadline)
        .map_err(|_| internal("image_publication_source_archive"))?;
    if !output.success || output.code != Some(0) || output.signal.is_some() {
        return Err(internal("image_publication_source_archive"));
    }
    Ok(output.stdout)
}

fn read_archive(
    bytes: &[u8],
    ids: &BTreeMap<String, (String, String)>,
    source: &str,
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let limited = decoder.take(MAX_EXPANDED_BYTES + 1);
    let mut archive = tar::Archive::new(limited);
    let mut files = BTreeMap::new();
    let mut prefix: Option<OsString> = None;
    let mut total = 0_u64;
    let mut global_seen = false;
    for item in archive
        .entries()
        .map_err(|_| internal("image_publication_source_archive"))?
    {
        deadline
            .remaining()
            .map_err(|_| internal("image_publication_deadline"))?;
        let mut entry = item.map_err(|_| internal("image_publication_source_archive"))?;
        if entry.header().entry_type().is_pax_global_extensions() {
            if global_seen || prefix.is_some() || entry.size() > 4096 {
                return Err(internal("image_publication_source_archive"));
            }
            let mut metadata = Vec::new();
            entry
                .read_to_end(&mut metadata)
                .map_err(|_| internal("image_publication_source_archive"))?;
            if metadata != format!("52 comment={source}\n").as_bytes() {
                return Err(internal("image_publication_source_archive"));
            }
            global_seen = true;
            continue;
        }
        read_entry(entry, ids, &mut prefix, &mut total, &mut files)?;
    }
    let mut remainder = archive.into_inner();
    std::io::copy(&mut remainder, &mut std::io::sink())
        .map_err(|_| internal("image_publication_source_archive"))?;
    if remainder.limit() == 0 {
        return Err(internal("image_publication_source_archive"));
    }
    if files.len() != ids.len() {
        return Err(internal("image_publication_source_inventory"));
    }
    Ok(files)
}

fn read_entry<R: Read>(
    item: tar::Entry<'_, R>,
    ids: &BTreeMap<String, (String, String)>,
    prefix: &mut Option<OsString>,
    total: &mut u64,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), OrchestratorError> {
    let mut entry = item;
    let path = entry
        .path()
        .map_err(|_| internal("image_publication_source_archive"))?
        .into_owned();
    let raw = path
        .to_str()
        .ok_or_else(|| internal("image_publication_source_archive"))?;
    let canonical = raw.strip_suffix('/').unwrap_or(raw);
    if canonical.is_empty()
        || canonical.starts_with('/')
        || canonical.contains('\\')
        || canonical
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(internal("image_publication_source_archive"));
    }
    let mut components = path.components();
    let Some(Component::Normal(top)) = components.next() else {
        return Err(internal("image_publication_source_archive"));
    };
    if prefix.as_ref().is_some_and(|prior| prior != top) {
        return Err(internal("image_publication_source_archive"));
    }
    prefix.get_or_insert_with(|| top.to_owned());
    let rest: &Path = components.as_path();
    if rest
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(internal("image_publication_source_archive"));
    }
    let relative = rest
        .to_str()
        .ok_or_else(|| internal("image_publication_source_archive"))?;
    if let Some((expected, mode)) = ids.get(relative) {
        if !entry.header().entry_type().is_file() || files.contains_key(relative) {
            return Err(internal("image_publication_source_archive"));
        }
        let archive_mode = entry
            .header()
            .mode()
            .map_err(|_| internal("image_publication_source_archive"))?;
        let expected_executable = mode == "100755";
        if archive_mode & !0o777 != 0 || (archive_mode & 0o111 != 0) != expected_executable {
            return Err(internal("image_publication_source_archive_mode"));
        }
        let size = entry.size();
        *total = total
            .checked_add(size)
            .filter(|total| *total <= MAX_SELECTED_BYTES)
            .ok_or_else(|| internal("image_publication_source_archive"))?;
        if size > MAX_FILE_BYTES {
            return Err(internal("image_publication_source_archive"));
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| internal("image_publication_source_archive"))?;
        if bytes.len() as u64 != size || git_blob_sha(&bytes)? != *expected {
            return Err(internal("image_publication_source_archive_digest"));
        }
        files.insert(relative.to_owned(), bytes);
    }
    Ok(())
}

fn git_blob_sha(bytes: &[u8]) -> Result<String, OrchestratorError> {
    let mut hash = Sha1::default();
    hash.update(format!("blob {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    let result = hash.try_finalize();
    if result.has_collision() {
        return Err(internal("image_publication_source_blob_collision"));
    }
    let mut output = String::with_capacity(40);
    for byte in result.hash() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(output)
}
