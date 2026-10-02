//! Portable Cargo scheduler state carried beside an exported action closure.

use crate::config::Config;
use eyre::{Context as _, Result, bail};
use mbx_cache_core::{CacheDigest, LocalCas};
use mbx_cache_store::{ExportAdditions, WorkspaceTarget};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, FileTimes};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

pub(crate) const ATTACHMENT: &str = "cargo-workspace-state-v1";
const VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u8,
    workspaces: Vec<WorkspaceState>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceState {
    workspace_root: PathBuf,
    signature: CacheDigest,
    inline_archive: CacheDigest,
    inline_files: Vec<FileMetadata>,
    references: Vec<FileReference>,
    symlinks: Vec<Symlink>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReference {
    path: PathBuf,
    source: FileSource,
    mode: u32,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileMetadata {
    path: PathBuf,
    mode: u32,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FileSource {
    Cas(CacheDigest),
    Mbx,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Symlink {
    path: PathBuf,
    target: PathBuf,
    directory: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct RestoreOutcome {
    pub(crate) files: u64,
    pub(crate) referenced_bytes: u64,
}

/// Capture every recorded Cargo target as a manifest plus an inline metadata tar.
pub(crate) fn capture(store: &Path, targets: &[WorkspaceTarget]) -> Result<ExportAdditions> {
    let executable = std::env::current_exe()?;
    let executable_digest = CacheDigest::blake3_file(&executable)?;
    let cas = LocalCas::new(store);
    let mut objects = BTreeSet::new();
    let mut workspaces = Vec::new();
    for target in targets {
        if !target.target_dir.is_dir() || !target.workspace_root.is_dir() {
            continue;
        }
        workspaces.push(capture_workspace(
            &cas,
            target,
            &executable_digest,
            &mut objects,
        )?);
    }
    if workspaces.is_empty() {
        return Ok(ExportAdditions::default());
    }
    let bytes = serde_json::to_vec(&Bundle {
        version: VERSION,
        workspaces,
    })?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    objects.insert(digest.clone());
    Ok(ExportAdditions {
        attachments: BTreeMap::from([(ATTACHMENT.to_owned(), digest)]),
        objects,
    })
}

/// Retain omitted supported workspaces while current captures replace the same root.
pub(crate) fn retain(
    store: &Path,
    mut additions: ExportAdditions,
    baseline: Option<&CacheDigest>,
) -> Result<ExportAdditions> {
    let Some(baseline) = baseline else {
        return Ok(additions);
    };
    let cas = LocalCas::new(store);
    let load = |digest: &CacheDigest| -> Result<Bundle> {
        let path = cas
            .find(digest)?
            .ok_or_else(|| eyre::eyre!("retained workspace attachment is missing"))?;
        let bundle: Bundle = serde_json::from_slice(&std::fs::read(path)?)?;
        if bundle.version != VERSION || bundle.workspaces.is_empty() {
            bail!("invalid retained workspace attachment");
        }
        for state in &bundle.workspaces {
            validate_state(state)?;
        }
        Ok(bundle)
    };
    let mut states = load(baseline)?
        .workspaces
        .into_iter()
        .map(|state| (state.workspace_root.clone(), state))
        .collect::<BTreeMap<_, _>>();
    if let Some(current) = additions.attachments.get(ATTACHMENT) {
        for state in load(current)?.workspaces {
            states.insert(state.workspace_root.clone(), state);
        }
    }
    let bundle = Bundle {
        version: VERSION,
        workspaces: states.into_values().collect(),
    };
    for state in &bundle.workspaces {
        additions.objects.insert(state.inline_archive.clone());
        for reference in &state.references {
            if let FileSource::Cas(digest) = &reference.source {
                additions.objects.insert(digest.clone());
            }
        }
    }
    let bytes = serde_json::to_vec(&bundle)?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    additions.objects.insert(digest.clone());
    additions.attachments.insert(ATTACHMENT.to_owned(), digest);
    Ok(additions)
}

fn capture_workspace(
    cas: &LocalCas,
    target: &WorkspaceTarget,
    executable_digest: &CacheDigest,
    objects: &mut BTreeSet<CacheDigest>,
) -> Result<WorkspaceState> {
    let temporary = tempfile::NamedTempFile::new_in(cas.root())?;
    let mut archive = tar::Builder::new(temporary.reopen()?);
    let mut references = Vec::new();
    let mut inline_files = Vec::new();
    let mut symlinks = Vec::new();
    for path in tree_entries(&target.target_dir)? {
        let relative = path.strip_prefix(&target.target_dir)?.to_path_buf();
        validate_relative_path(&relative)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        let kind = metadata.file_type();
        if kind.is_dir() {
            archive.append_dir(&relative, &path)?;
        } else if kind.is_symlink() {
            let link = std::fs::read_link(&path)?;
            validate_link(&relative, &link)?;
            symlinks.push(Symlink {
                path: relative,
                target: link,
                directory: path.metadata().is_ok_and(|metadata| metadata.is_dir()),
            });
        } else if kind.is_file() {
            let digest = CacheDigest::blake3_file(&path)?;
            let source = if digest == *executable_digest {
                Some(FileSource::Mbx)
            } else if cas.path_for(&digest)?.is_file() {
                objects.insert(digest.clone());
                Some(FileSource::Cas(digest))
            } else {
                None
            };
            if let Some(source) = source {
                let (modified_secs, modified_nanos) = modified_parts(&metadata);
                references.push(FileReference {
                    path: relative,
                    source,
                    mode: file_mode(&metadata),
                    modified_secs,
                    modified_nanos,
                });
            } else {
                archive.append_path_with_name(&path, &relative)?;
                let (modified_secs, modified_nanos) = modified_parts(&metadata);
                inline_files.push(FileMetadata {
                    path: relative,
                    mode: file_mode(&metadata),
                    modified_secs,
                    modified_nanos,
                });
            }
        }
    }
    archive.finish()?;
    drop(archive);
    let inline_archive = CacheDigest::blake3_file(temporary.path())?;
    cas.store_file(&inline_archive, temporary.path())?;
    objects.insert(inline_archive.clone());
    Ok(WorkspaceState {
        workspace_root: target.workspace_root.clone(),
        signature: workspace_signature(&target.workspace_root)?,
        inline_archive,
        inline_files,
        references,
        symlinks,
    })
}

/// Return stable semantic records carried by a workspace-state attachment.
///
/// The inventory is keyed by a workspace signature, a digest of that
/// workspace's complete semantic entry map, and a normalized relative path.
/// The workspace root and all timestamp metadata stay out of both keys and
/// values.  A workspace digest is the marker, so a deletion changes the
/// marker even when the deleted entry is absent from the after-inventory.  An
/// absent attachment has an empty inventory.
pub(crate) fn semantic_inventory(
    store: &Path,
    attachment: Option<&CacheDigest>,
) -> Result<BTreeMap<String, serde_json::Value>> {
    let Some(attachment) = attachment else {
        return Ok(BTreeMap::new());
    };
    attachment.validate()?;
    let cas = LocalCas::new(store);
    let bundle_path = cas
        .find(attachment)?
        .ok_or_else(|| eyre::eyre!("workspace-state attachment is missing"))?;
    let bundle: Bundle = serde_json::from_slice(&std::fs::read(bundle_path)?)?;
    if bundle.version != VERSION || bundle.workspaces.is_empty() {
        bail!("unsupported or invalid Cargo workspace-state attachment");
    }

    let mut workspaces = bundle
        .workspaces
        .iter()
        .map(|state| semantic_workspace(&cas, state))
        .collect::<Result<Vec<_>>>()?;
    workspaces.sort_by(|left, right| {
        left.signature
            .key()
            .cmp(&right.signature.key())
            .then_with(|| left.digest.key().cmp(&right.digest.key()))
    });

    let mut inventory = BTreeMap::new();
    for workspace in workspaces {
        let prefix = format!(
            "workspace/{}/{}/{}/state/{}/{}/{}",
            workspace.signature.algorithm,
            workspace.signature.hash,
            workspace.signature.size,
            workspace.digest.algorithm,
            workspace.digest.hash,
            workspace.digest.size,
        );
        inventory.insert(
            prefix.clone(),
            serde_json::json!({
                "type": "workspace",
                "content": {
                    "kind": "digest",
                    "digest": workspace.digest,
                },
            }),
        );
        for (path, value) in workspace.entries {
            inventory.insert(format!("{prefix}/{path}"), value);
        }
    }
    Ok(inventory)
}

/// Validate an owner-produced semantic inventory before it becomes a
/// comparison baseline.
pub(crate) fn validate_semantic_inventory(
    inventory: &BTreeMap<String, serde_json::Value>,
) -> Result<()> {
    let mut markers = BTreeMap::<(CacheDigest, CacheDigest), CacheDigest>::new();
    let mut entries =
        BTreeMap::<(CacheDigest, CacheDigest), BTreeMap<String, serde_json::Value>>::new();
    for (key, value) in inventory {
        let parsed = parse_semantic_key(key)?;
        let marker = validate_semantic_value(&parsed, value)?;
        let identity = (parsed.signature, parsed.state);
        match parsed.path {
            Some(path) => {
                entries
                    .entry(identity)
                    .or_default()
                    .insert(path, value.clone());
            }
            None => {
                let digest = marker.ok_or_else(|| {
                    eyre::eyre!("workspace semantic marker is missing its digest")
                })?;
                if markers.insert(identity, digest).is_some() {
                    bail!("workspace semantic inventory contains a duplicate marker");
                }
            }
        }
    }
    for (identity, marker_digest) in &markers {
        let encoded = match entries.get(identity) {
            Some(entries) => serde_json::to_vec(entries)?,
            None => serde_json::to_vec(&BTreeMap::<String, serde_json::Value>::new())?,
        };
        let computed = CacheDigest::blake3(&encoded);
        if computed != *marker_digest {
            bail!("workspace semantic marker does not match its entries");
        }
    }
    for identity in entries.keys() {
        if !markers.contains_key(identity) {
            bail!("workspace semantic entry has no marker");
        }
    }
    Ok(())
}

struct ParsedSemanticKey {
    signature: CacheDigest,
    state: CacheDigest,
    path: Option<String>,
}

fn parse_semantic_key(key: &str) -> Result<ParsedSemanticKey> {
    if key.is_empty() || key.contains('\\') {
        bail!("workspace semantic key is not canonical: {key}");
    }
    let parts = key.split('/').collect::<Vec<_>>();
    if parts.len() < 8
        || parts[0] != "workspace"
        || parts[4] != "state"
        || parts.iter().any(|part| part.is_empty())
    {
        bail!("workspace semantic key is not canonical: {key}");
    }
    let signature = semantic_key_digest(&parts[1..4], key)?;
    let state = semantic_key_digest(&parts[5..8], key)?;
    let path = if parts.len() == 8 {
        None
    } else {
        let path = parts[8..].join("/");
        let normalized = normalized_relative_path(Path::new(&path))?;
        if normalized != path {
            bail!("workspace semantic key is not canonical: {key}");
        }
        Some(path)
    };
    Ok(ParsedSemanticKey {
        signature,
        state,
        path,
    })
}

fn semantic_key_digest(parts: &[&str], key: &str) -> Result<CacheDigest> {
    let size = parts[2]
        .parse::<u64>()
        .map_err(|_| eyre::eyre!("workspace semantic key has an invalid size: {key}"))?;
    if size.to_string() != parts[2] {
        bail!("workspace semantic key is not canonical: {key}");
    }
    let digest = CacheDigest {
        algorithm: parts[0].to_owned(),
        hash: parts[1].to_owned(),
        size,
    };
    digest.validate()?;
    Ok(digest)
}

fn validate_semantic_value(
    key: &ParsedSemanticKey,
    value: &serde_json::Value,
) -> Result<Option<CacheDigest>> {
    let object = value
        .as_object()
        .ok_or_else(|| eyre::eyre!("workspace semantic entry is not an object"))?;
    let kind = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| eyre::eyre!("workspace semantic entry has no type"))?;
    match (key.path.as_deref(), kind) {
        (None, "workspace") => {
            require_semantic_fields(object, &["type", "content"])?;
            match validate_semantic_content(object.get("content"), false)? {
                SemanticContent::Digest(digest) if digest == key.state => Ok(Some(digest)),
                SemanticContent::Digest(_) => {
                    bail!("workspace semantic marker digest does not match its key")
                }
                SemanticContent::Mbx => bail!("workspace semantic marker cannot use mbx content"),
            }
        }
        (Some(_), "file") => {
            require_semantic_fields(object, &["type", "content", "mode"])?;
            validate_semantic_mode(object)?;
            validate_semantic_content(object.get("content"), true)?;
            Ok(None)
        }
        (Some(_), "directory") => {
            require_semantic_fields(object, &["type", "mode"])?;
            validate_semantic_mode(object)?;
            Ok(None)
        }
        (Some(path), "symlink") => {
            require_semantic_fields(object, &["type", "target", "directory"])?;
            let target = object
                .get("target")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| eyre::eyre!("workspace semantic symlink has no target"))?;
            if target.contains('\\') {
                bail!("workspace semantic symlink target is not canonical");
            }
            let target_path = Path::new(target);
            if normalized_link_target(target_path)? != target
                || !object
                    .get("directory")
                    .is_some_and(serde_json::Value::is_boolean)
            {
                bail!("workspace semantic symlink is invalid");
            }
            validate_link(Path::new(path), target_path)?;
            Ok(None)
        }
        (None, _) => bail!("workspace semantic marker has an invalid type: {kind}"),
        (Some(_), _) => bail!("workspace semantic entry has an invalid type: {kind}"),
    }
}

fn require_semantic_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    fields: &[&str],
) -> Result<()> {
    if object.len() != fields.len() || object.keys().any(|key| !fields.contains(&key.as_str())) {
        bail!("workspace semantic entry has unknown or missing fields");
    }
    Ok(())
}

fn validate_semantic_mode(object: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
    let mode = object
        .get("mode")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| eyre::eyre!("workspace semantic entry has an invalid mode"))?;
    u32::try_from(mode).map_err(|_| eyre::eyre!("workspace semantic entry has an invalid mode"))?;
    Ok(())
}

enum SemanticContent {
    Digest(CacheDigest),
    Mbx,
}

fn validate_semantic_content(
    value: Option<&serde_json::Value>,
    allow_mbx: bool,
) -> Result<SemanticContent> {
    let object = value
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| eyre::eyre!("workspace semantic entry has invalid content"))?;
    let kind = object
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| eyre::eyre!("workspace semantic entry has invalid content kind"))?;
    match kind {
        "digest" => {
            require_semantic_fields(object, &["kind", "digest"])?;
            let digest: CacheDigest = serde_json::from_value(
                object
                    .get("digest")
                    .cloned()
                    .ok_or_else(|| eyre::eyre!("workspace semantic entry has no digest"))?,
            )?;
            digest.validate()?;
            Ok(SemanticContent::Digest(digest))
        }
        "mbx" if allow_mbx => {
            require_semantic_fields(object, &["kind"])?;
            Ok(SemanticContent::Mbx)
        }
        "mbx" => bail!("workspace semantic marker cannot use mbx content"),
        _ => bail!("workspace semantic entry has an invalid content kind: {kind}"),
    }
}

struct SemanticWorkspace {
    signature: CacheDigest,
    digest: CacheDigest,
    entries: BTreeMap<String, serde_json::Value>,
}

fn semantic_workspace(cas: &LocalCas, state: &WorkspaceState) -> Result<SemanticWorkspace> {
    validate_state(state)?;
    let mut entries = BTreeMap::new();
    let modes = inline_file_modes(state)?;
    semantic_inline_entries(cas, state, &modes, &mut entries)?;
    for reference in &state.references {
        let path = normalized_relative_path(&reference.path)?;
        let content = match &reference.source {
            FileSource::Cas(digest) => serde_json::json!({
                "kind": "digest",
                "digest": digest,
            }),
            FileSource::Mbx => serde_json::json!({"kind": "mbx"}),
        };
        insert_semantic_entry(
            &mut entries,
            path,
            serde_json::json!({
                "type": "file",
                "content": content,
                "mode": reference.mode,
            }),
        )?;
    }
    for link in &state.symlinks {
        let path = normalized_relative_path(&link.path)?;
        let target = normalized_link_target(&link.target)?;
        insert_semantic_entry(
            &mut entries,
            path,
            serde_json::json!({
                "type": "symlink",
                "target": target,
                "directory": link.directory,
            }),
        )?;
    }
    // Cargo's compiler-query cache fingerprints wrapper paths and filesystem
    // timestamps. It is restored byte-for-byte, but is outside useful compiled
    // actions and Cargo unit scheduler state. See Cargo rustc.rs at 797e8a9b.
    entries.remove(".rustc_info.json");
    let encoded = serde_json::to_vec(&entries)?;
    let digest = CacheDigest::blake3(&encoded);
    Ok(SemanticWorkspace {
        signature: state.signature.clone(),
        digest,
        entries,
    })
}

fn inline_file_modes(state: &WorkspaceState) -> Result<BTreeMap<String, u32>> {
    let mut modes = BTreeMap::new();
    for metadata in &state.inline_files {
        let path = normalized_relative_path(&metadata.path)?;
        if modes.insert(path.clone(), metadata.mode).is_some() {
            bail!("workspace-state contains duplicate inline file {path}");
        }
    }
    Ok(modes)
}

fn semantic_inline_entries(
    cas: &LocalCas,
    state: &WorkspaceState,
    modes: &BTreeMap<String, u32>,
    entries: &mut BTreeMap<String, serde_json::Value>,
) -> Result<()> {
    let archive_path = cas
        .find(&state.inline_archive)?
        .ok_or_else(|| eyre::eyre!("workspace-state inline archive is missing"))?;
    let mut archive = tar::Archive::new(File::open(archive_path)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let normalized = normalized_relative_path(&path)?;
        let mode = entry.header().mode()?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            insert_semantic_entry(
                entries,
                normalized,
                serde_json::json!({
                    "type": "directory",
                    "mode": mode,
                }),
            )?;
        } else if kind.is_file() || kind.is_gnu_sparse() {
            let temporary = tempfile::NamedTempFile::new()?;
            {
                let mut output = temporary.reopen()?;
                std::io::copy(&mut entry, &mut output)?;
            }
            let digest = CacheDigest::blake3_file(temporary.path())?;
            insert_semantic_entry(
                entries,
                normalized.clone(),
                serde_json::json!({
                    "type": "file",
                    "content": {
                        "kind": "digest",
                        "digest": digest,
                    },
                    "mode": modes.get(&normalized).copied().unwrap_or(mode),
                }),
            )?;
        } else {
            bail!("workspace-state archive contains a non-file entry");
        }
    }
    Ok(())
}

fn insert_semantic_entry(
    entries: &mut BTreeMap<String, serde_json::Value>,
    path: String,
    value: serde_json::Value,
) -> Result<()> {
    if entries.insert(path.clone(), value).is_some() {
        bail!("workspace-state contains duplicate entry {path}");
    }
    Ok(())
}

fn normalized_relative_path(path: &Path) -> Result<String> {
    validate_relative_path(path)?;
    normalized_path(path)
}

fn normalized_link_target(path: &Path) -> Result<String> {
    if path.is_absolute() {
        bail!("workspace state contains an absolute link target");
    }
    normalized_path(path)
}

fn normalized_path(path: &Path) -> Result<String> {
    let mut normalized = String::new();
    for component in path.components() {
        let component = match component {
            Component::Normal(component) => component,
            Component::CurDir => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push('.');
                continue;
            }
            Component::ParentDir => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push_str("..");
                continue;
            }
            _ => bail!("workspace state contains an unsafe path {}", path.display()),
        };
        let component = component
            .to_str()
            .ok_or_else(|| eyre::eyre!("workspace state path is not valid UTF-8"))?;
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    if normalized.is_empty() {
        bail!("workspace state contains an empty path");
    }
    Ok(normalized)
}

/// Restore the state matching the current Cargo workspace into an empty target.
pub(crate) fn restore(
    config: &Config,
    store: &Path,
    attachment: &CacheDigest,
    workspace_root: &Path,
    target_dir: &Path,
    target_requested: bool,
) -> Result<Option<RestoreOutcome>> {
    let cas = LocalCas::new(store);
    let bundle_path = cas
        .find(attachment)?
        .ok_or_else(|| eyre::eyre!("workspace-state attachment is missing"))?;
    let bundle: Bundle = serde_json::from_slice(&std::fs::read(bundle_path)?)?;
    if bundle.version != VERSION || bundle.workspaces.is_empty() {
        bail!("unsupported or invalid Cargo workspace-state attachment");
    }
    let signature = workspace_signature(workspace_root)?;
    let Some(state) = bundle
        .workspaces
        .iter()
        .find(|state| state.workspace_root == workspace_root)
        .or_else(|| {
            let mut matches = bundle
                .workspaces
                .iter()
                .filter(|state| state.signature == signature);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        })
    else {
        return Ok(None);
    };
    validate_state(state)?;
    let destination = crate::target::place(config, workspace_root, target_dir, target_requested)
        .unwrap_or_else(|| target_dir.to_path_buf());
    if destination.exists() && std::fs::read_dir(&destination)?.next().is_some() {
        log::debug!(
            "leaving the non-empty target directory {} alone",
            destination.display()
        );
        return Ok(None);
    }
    let parent = destination
        .parent()
        .ok_or_else(|| eyre::eyre!("target directory has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".mbx-workspace-state-")
        .tempdir_in(parent)?;
    let staged_target = staging.path().join("target");
    std::fs::create_dir(&staged_target)?;
    unpack_inline(&cas, &state.inline_archive, &staged_target)?;
    for metadata in &state.inline_files {
        set_file_metadata(
            &staged_target.join(&metadata.path),
            metadata.mode,
            metadata.modified_secs,
            metadata.modified_nanos,
        )?;
    }
    let executable = std::env::current_exe()?;
    let bytes = materialize_references(&cas, &executable, &staged_target, &state.references)?;
    restore_symlinks(&staged_target, &state.symlinks)?;
    if destination.exists() {
        std::fs::remove_dir(&destination).wrap_err_with(|| {
            format!(
                "could not replace empty target directory {}",
                destination.display()
            )
        })?;
    }
    std::fs::rename(&staged_target, &destination)?;
    crate::target::touch_managed(config, workspace_root, target_dir);
    Ok(Some(RestoreOutcome {
        files: state.references.len() as u64,
        referenced_bytes: bytes,
    }))
}

fn unpack_inline(cas: &LocalCas, digest: &CacheDigest, destination: &Path) -> Result<()> {
    let path = cas
        .find(digest)?
        .ok_or_else(|| eyre::eyre!("workspace-state inline archive is missing"))?;
    let mut archive = tar::Archive::new(File::open(path)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        validate_relative_path(&path)?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() && !kind.is_gnu_sparse() {
            bail!("workspace-state archive contains a non-file entry");
        }
        if !entry.unpack_in(destination)? {
            bail!("workspace-state archive contains an unsafe path");
        }
    }
    Ok(())
}

fn materialize_references(
    cas: &LocalCas,
    executable: &Path,
    destination: &Path,
    references: &[FileReference],
) -> Result<u64> {
    let sources = references
        .iter()
        .map(|reference| {
            let source = match &reference.source {
                FileSource::Cas(digest) => cas.path_for(digest)?,
                FileSource::Mbx => executable.to_path_buf(),
            };
            let size = std::fs::metadata(&source)?.len();
            Ok((reference, source, size))
        })
        .collect::<Result<Vec<_>>>()?;
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(sources.len().max(1));
    let next = AtomicUsize::new(0);
    let error = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((reference, source, _)) = sources.get(index) else {
                        break;
                    };
                    if error.lock().unwrap().is_some() {
                        break;
                    }
                    let result = restore_reference(destination, reference, source);
                    if let Err(found) = result {
                        *error.lock().unwrap() = Some(found);
                        break;
                    }
                }
            });
        }
    });
    if let Some(error) = error.into_inner().unwrap() {
        return Err(error);
    }
    Ok(sources.iter().map(|(_, _, size)| size).sum())
}

fn restore_reference(root: &Path, reference: &FileReference, source: &Path) -> Result<()> {
    let destination = root.join(&reference.path);
    let copied = reflink_copy::reflink_or_copy(source, &destination)?;
    let _ = copied;
    set_file_metadata(
        &destination,
        reference.mode,
        reference.modified_secs,
        reference.modified_nanos,
    )
}

fn restore_symlinks(root: &Path, links: &[Symlink]) -> Result<()> {
    for link in links {
        let destination = root.join(&link.path);
        create_symlink(&link.target, &destination, link.directory)?;
    }
    Ok(())
}

fn validate_state(state: &WorkspaceState) -> Result<()> {
    state.signature.validate()?;
    state.inline_archive.validate()?;
    for metadata in &state.inline_files {
        validate_relative_path(&metadata.path)?;
    }
    for reference in &state.references {
        validate_relative_path(&reference.path)?;
        if let FileSource::Cas(digest) = &reference.source {
            digest.validate()?;
        }
    }
    for link in &state.symlinks {
        validate_relative_path(&link.path)?;
        validate_link(&link.path, &link.target)?;
    }
    Ok(())
}

fn tree_entries(root: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = std::fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

fn workspace_signature(root: &Path) -> Result<CacheDigest> {
    let mut bytes = b"cargo-workspace-state-v1\0".to_vec();
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = root.join(name);
        if path.is_file() {
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(&std::fs::read(path)?);
            bytes.push(0);
        }
    }
    Ok(CacheDigest::blake3(&bytes))
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("workspace state contains unsafe path {}", path.display());
    }
    Ok(())
}

fn validate_link(path: &Path, target: &Path) -> Result<()> {
    if target.is_absolute() {
        bail!("workspace state contains unsafe link {}", path.display());
    }
    let mut depth = path
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => bail!("workspace state contains unsafe link {}", path.display()),
        }
    }
    Ok(())
}

fn modified_parts(metadata: &std::fs::Metadata) -> (u64, u32) {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
        .unwrap_or_default()
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode()
}

#[cfg(not(unix))]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn set_file_metadata(path: &Path, mode: u32, secs: u64, nanos: u32) -> Result<()> {
    if nanos >= 1_000_000_000 {
        bail!("workspace state contains an invalid timestamp");
    }
    let modified = UNIX_EPOCH
        .checked_add(std::time::Duration::new(secs, nanos))
        .ok_or_else(|| eyre::eyre!("workspace state contains an out-of-range timestamp"))?;
    #[cfg(unix)]
    File::options()
        .read(true)
        .open(path)?
        .set_times(FileTimes::new().set_modified(modified))
        .wrap_err_with(|| format!("could not restore the timestamp of {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .wrap_err_with(|| format!("could not restore the mode of {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions.clone())?;
        File::options()
            .write(true)
            .open(path)?
            .set_times(FileTimes::new().set_modified(modified))
            .wrap_err_with(|| format!("could not restore the timestamp of {}", path.display()))?;
        permissions.set_readonly(mode == 1);
        std::fs::set_permissions(path, permissions)
            .wrap_err_with(|| format!("could not restore the permissions of {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path, _directory: bool) -> Result<()> {
    std::os::unix::fs::symlink(target, destination)?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(target: &Path, destination: &Path, directory: bool) -> Result<()> {
    if directory {
        std::os::windows::fs::symlink_dir(target, destination)?;
    } else {
        std::os::windows::fs::symlink_file(target, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        target: WorkspaceTarget,
        inline: PathBuf,
    }

    fn fixture(root: &Path, name: &str, reference: &[u8]) -> Result<Fixture> {
        let workspace_root = root.join(name);
        let target_dir = workspace_root.join("target");
        fs::create_dir_all(target_dir.join("nested/deps"))?;
        fs::write(
            workspace_root.join("Cargo.toml"),
            b"[package]\nname = \"fixture\"\n",
        )?;
        let inline = target_dir.join("nested/inline.txt");
        fs::write(&inline, b"inline contents")?;
        fs::write(target_dir.join("reference.bin"), reference)?;
        let reference_digest = CacheDigest::blake3(reference);
        LocalCas::new(root).store_bytes(&reference_digest, reference)?;
        let executable = std::env::current_exe()?;
        fs::copy(executable, target_dir.join("mbx-placeholder"))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink("nested/inline.txt", target_dir.join("link"))?;
        Ok(Fixture {
            target: WorkspaceTarget {
                workspace_root,
                target_dir,
            },
            inline,
        })
    }

    fn attachment(additions: &ExportAdditions) -> Result<CacheDigest> {
        additions
            .attachments
            .get(ATTACHMENT)
            .cloned()
            .ok_or_else(|| eyre::eyre!("fixture did not produce a workspace attachment"))
    }

    fn find_entry<'a>(
        inventory: &'a BTreeMap<String, serde_json::Value>,
        suffix: &str,
    ) -> Result<&'a serde_json::Value> {
        inventory
            .iter()
            .find_map(|(key, value)| key.ends_with(suffix).then_some(value))
            .ok_or_else(|| eyre::eyre!("missing semantic inventory entry {suffix}"))
    }

    #[test]
    fn semantic_inventory_preserves_content_shape_and_ignores_root_and_time() -> Result<()> {
        let store = tempfile::tempdir()?;
        let fixture = fixture(store.path(), "workspace", b"reference contents")?;
        let first = capture(store.path(), std::slice::from_ref(&fixture.target))?;
        let first_inventory = semantic_inventory(store.path(), Some(&attachment(&first)?))?;
        let first_json = serde_json::to_string(&first_inventory)?;
        assert!(!first_json.contains(&fixture.target.workspace_root.display().to_string()));
        assert!(!first_json.contains("modified_secs"));

        let inline = first_inventory
            .iter()
            .find(|(key, _)| key.ends_with("/nested/inline.txt"))
            .map(|(_, value)| value)
            .ok_or_else(|| eyre::eyre!("inline file is missing"))?;
        assert_eq!(inline["type"], "file");
        assert_eq!(inline["content"]["kind"], "digest");
        assert_eq!(inline["content"]["digest"]["algorithm"], "blake3");
        assert!(inline["mode"].is_u64());
        assert_eq!(
            find_entry(&first_inventory, "/nested")?["type"],
            "directory"
        );
        let reference = find_entry(&first_inventory, "/reference.bin")?;
        assert_eq!(reference["content"]["kind"], "digest");
        let mbx = find_entry(&first_inventory, "/mbx-placeholder")?;
        assert_eq!(mbx["content"]["kind"], "mbx");
        assert!(mbx["content"].get("digest").is_none());
        let marker = first_inventory
            .values()
            .find(|value| value["type"] == "workspace")
            .ok_or_else(|| eyre::eyre!("workspace marker is missing"))?;
        assert_eq!(marker["content"]["kind"], "digest");

        filetime::set_file_mtime(&fixture.inline, filetime::FileTime::from_unix_time(1, 2))?;
        let second = capture(store.path(), std::slice::from_ref(&fixture.target))?;
        let second_inventory = semantic_inventory(store.path(), Some(&attachment(&second)?))?;
        assert_eq!(first_inventory, second_inventory);
        #[cfg(unix)]
        assert_eq!(
            find_entry(&first_inventory, "/link")?["target"],
            "nested/inline.txt"
        );
        Ok(())
    }

    #[test]
    fn semantic_workspace_identity_is_stable_when_same_signature_sibling_is_omitted() -> Result<()>
    {
        let store = tempfile::tempdir()?;
        let first = fixture(store.path(), "first", b"reference contents")?;
        let second = fixture(store.path(), "second", b"reference contents")?;
        fs::write(&second.inline, b"second workspace contents")?;
        let both = capture(store.path(), &[first.target.clone(), second.target.clone()])?;
        let only_second = capture(store.path(), std::slice::from_ref(&second.target))?;
        let all = semantic_inventory(store.path(), Some(&attachment(&both)?))?;
        let subset = semantic_inventory(store.path(), Some(&attachment(&only_second)?))?;
        for (key, value) in subset {
            assert_eq!(all.get(&key), Some(&value), "subset entry moved: {key}");
        }
        Ok(())
    }
    #[test]
    fn semantic_inventory_excludes_only_root_compiler_probe_cache() -> Result<()> {
        let store = tempfile::tempdir()?;
        let fixture = fixture(store.path(), "workspace", b"reference contents")?;
        let probe = fixture.target.target_dir.join(".rustc_info.json");
        std::fs::write(&probe, br#"{"rustc_fingerprint":1}"#)?;
        let first = capture(store.path(), std::slice::from_ref(&fixture.target))?;
        let before = semantic_inventory(store.path(), Some(&attachment(&first)?))?;
        std::fs::write(&probe, br#"{"rustc_fingerprint":2}"#)?;
        let second = capture(store.path(), std::slice::from_ref(&fixture.target))?;
        let after = semantic_inventory(store.path(), Some(&attachment(&second)?))?;
        assert_eq!(before, after);
        let cas = LocalCas::new(store.path());
        let bundle: Bundle =
            serde_json::from_slice(&std::fs::read(cas.path_for(&attachment(&second)?)?)?)?;
        assert!(
            bundle.workspaces[0]
                .inline_files
                .iter()
                .any(|entry| entry.path == Path::new(".rustc_info.json"))
        );
        std::fs::write(
            fixture.target.target_dir.join("nested/.rustc_info.json"),
            b"actual nested content",
        )?;
        let nested = capture(store.path(), std::slice::from_ref(&fixture.target))?;
        assert_ne!(
            after,
            semantic_inventory(store.path(), Some(&attachment(&nested)?))?
        );
        Ok(())
    }
    #[test]
    fn retained_workspace_union_replaces_current_root_and_keeps_omitted_roots() -> Result<()> {
        let store = tempfile::tempdir()?;
        let first = fixture(store.path(), "first", b"first output")?;
        let second = fixture(store.path(), "second", b"second output")?;
        let baseline = capture(store.path(), &[first.target.clone(), second.target.clone()])?;
        let baseline_inventory = semantic_inventory(store.path(), Some(&attachment(&baseline)?))?;
        std::fs::write(&first.inline, b"updated current state")?;
        let current = capture(store.path(), std::slice::from_ref(&first.target))?;
        let merged = retain(store.path(), current, Some(&attachment(&baseline)?))?;
        let cas = LocalCas::new(store.path());
        let bundle: Bundle =
            serde_json::from_slice(&std::fs::read(cas.path_for(&attachment(&merged)?)?)?)?;
        assert_eq!(bundle.workspaces.len(), 2);
        assert_eq!(
            bundle
                .workspaces
                .iter()
                .filter(|state| state.workspace_root == first.target.workspace_root)
                .count(),
            1
        );
        let inventory = semantic_inventory(store.path(), Some(&attachment(&merged)?))?;
        let second_only = capture(store.path(), std::slice::from_ref(&second.target))?;
        let omitted = semantic_inventory(store.path(), Some(&attachment(&second_only)?))?;
        assert!(
            omitted
                .iter()
                .all(|(key, value)| inventory.get(key) == Some(value))
        );
        assert_ne!(baseline_inventory, inventory);
        let unchanged = retain(
            store.path(),
            ExportAdditions::default(),
            Some(&attachment(&merged)?),
        )?;
        assert_eq!(
            inventory,
            semantic_inventory(store.path(), Some(&attachment(&unchanged)?))?
        );
        let retained_inline = bundle.workspaces[0].inline_archive.clone();
        std::fs::write(cas.path_for(&retained_inline)?, b"tampered")?;
        assert!(semantic_inventory(store.path(), Some(&attachment(&unchanged)?)).is_err());
        Ok(())
    }

    #[test]
    fn semantic_inventory_validation_rejects_unknown_fields_and_unsafe_links() -> Result<()> {
        let store = tempfile::tempdir()?;
        let fixture = fixture(store.path(), "workspace", b"reference contents")?;
        let additions = capture(store.path(), std::slice::from_ref(&fixture.target))?;
        let inventory = semantic_inventory(store.path(), Some(&attachment(&additions)?))?;
        validate_semantic_inventory(&inventory)?;

        let marker_key = inventory
            .iter()
            .find(|(_, value)| value["type"] == "workspace")
            .map(|(key, _)| key.clone())
            .ok_or_else(|| eyre::eyre!("workspace marker is missing"))?;
        let mut unknown = inventory.clone();
        unknown
            .get_mut(&marker_key)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| eyre::eyre!("workspace marker is not an object"))?
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(validate_semantic_inventory(&unknown).is_err());

        #[cfg(unix)]
        {
            let link_key = inventory
                .keys()
                .find(|key| key.ends_with("/link"))
                .cloned()
                .ok_or_else(|| eyre::eyre!("symlink is missing"))?;
            let mut unsafe_link = inventory;
            unsafe_link
                .get_mut(&link_key)
                .and_then(serde_json::Value::as_object_mut)
                .ok_or_else(|| eyre::eyre!("symlink is not an object"))?
                .insert("target".to_owned(), serde_json::json!("../../outside"));
            assert!(validate_semantic_inventory(&unsafe_link).is_err());
        }
        Ok(())
    }
}
