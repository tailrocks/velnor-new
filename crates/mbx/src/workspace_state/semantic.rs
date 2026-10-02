use super::*;

pub(super) struct SemanticWorkspace {
    pub(super) signature: CacheDigest,
    pub(super) digest: CacheDigest,
    pub(super) entries: BTreeMap<String, serde_json::Value>,
}

pub(super) fn semantic_workspace(
    cas: &LocalCas,
    state: &WorkspaceState,
) -> Result<SemanticWorkspace> {
    validate_state(state)?;
    let mut entries = BTreeMap::new();
    for tree in &state.trees {
        let mut tree_entries = BTreeMap::new();
        let modes = inline_file_modes(tree)?;
        semantic_inline_entries(cas, tree, &modes, &mut tree_entries)?;
        for reference in &tree.references {
            let path = normalized_relative_path(&reference.path)?;
            let content = match &reference.source {
                FileSource::Cas(digest) => {
                    cas.find(digest)?.ok_or_else(|| {
                        eyre::eyre!("workspace-state referenced object is missing")
                    })?;
                    serde_json::json!({"kind": "digest", "digest": digest})
                }
                FileSource::Mbx => serde_json::json!({"kind": "mbx"}),
            };
            insert_semantic_entry(
                &mut tree_entries,
                path,
                serde_json::json!({
                    "type": "file",
                    "content": content,
                    "mode": reference.mode,
                }),
            )?;
        }
        for link in &tree.symlinks {
            let path = normalized_relative_path(&link.path)?;
            let target = normalized_link_target(&link.target)?;
            insert_semantic_entry(
                &mut tree_entries,
                path,
                serde_json::json!({
                    "type": "symlink",
                    "target": target,
                    "directory": link.directory,
                }),
            )?;
        }
        for path in tree_entries.keys() {
            let mut ancestor = Path::new(path).parent();
            while let Some(parent) = ancestor {
                if let Some(value) = tree_entries.get(&normalized_path(parent).unwrap_or_default())
                    && value["type"] != "directory"
                {
                    bail!("workspace-state entry has a non-directory ancestor");
                }
                ancestor = parent.parent();
            }
        }
        // Cargo's compiler-query cache fingerprints wrapper paths and filesystem
        // timestamps. It is restored byte-for-byte, but is outside useful compiled
        // actions and Cargo unit scheduler state. See Cargo rustc.rs at 797e8a9b.
        if tree.role == RootRole::Build
            || state.cargo_roots.target_dir == state.cargo_roots.build_dir
        {
            tree_entries.remove(".rustc_info.json");
        }
        let role = match tree.role {
            RootRole::Target => "target",
            RootRole::Build => "build",
        };
        entries.insert(role.to_owned(), serde_json::json!({"type": "root"}));
        for (path, value) in tree_entries {
            entries.insert(format!("{role}/{path}"), value);
        }
    }
    let encoded = serde_json::to_vec(&entries)?;
    let digest = CacheDigest::blake3(&encoded);
    Ok(SemanticWorkspace {
        signature: state.signature.clone(),
        digest,
        entries,
    })
}

pub(super) fn inline_file_modes(state: &RootTree) -> Result<BTreeMap<String, u32>> {
    let mut modes = BTreeMap::new();
    for metadata in &state.inline_files {
        let path = normalized_relative_path(&metadata.path)?;
        if modes.insert(path.clone(), metadata.mode).is_some() {
            bail!("workspace-state contains duplicate inline file {path}");
        }
    }
    Ok(modes)
}

pub(super) fn semantic_inline_entries(
    cas: &LocalCas,
    state: &RootTree,
    modes: &BTreeMap<String, u32>,
    entries: &mut BTreeMap<String, serde_json::Value>,
) -> Result<()> {
    let archive_path = cas
        .find(&state.inline_archive)?
        .ok_or_else(|| eyre::eyre!("workspace-state inline archive is missing"))?;
    let mut archive = tar::Archive::new(File::open(archive_path)?);
    let mut seen_files = BTreeSet::new();
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
            seen_files.insert(normalized.clone());
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
                    "mode": modes.get(&normalized).copied().ok_or_else(|| eyre::eyre!("inline file has no metadata"))?,
                }),
            )?;
        } else {
            bail!("workspace-state archive contains a non-file entry");
        }
    }
    if seen_files != modes.keys().cloned().collect() {
        bail!("inline metadata does not match archive files");
    }
    Ok(())
}

pub(super) fn insert_semantic_entry(
    entries: &mut BTreeMap<String, serde_json::Value>,
    path: String,
    value: serde_json::Value,
) -> Result<()> {
    if entries.insert(path.clone(), value).is_some() {
        bail!("workspace-state contains duplicate entry {path}");
    }
    Ok(())
}
