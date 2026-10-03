use super::*;

/// Validate native owner descriptors against bytes already in the Build role.
pub(super) fn validate_inventory(
    cas: &LocalCas,
    state: &WorkspaceState,
    entries: &BTreeMap<String, serde_json::Value>,
) -> Result<()> {
    let role = if state.cargo_roots.target_dir == state.cargo_roots.build_dir {
        "target"
    } else {
        "build"
    };
    for snapshot in &state.owned_out_dirs {
        let source = normalized_relative_path(&snapshot.source)?;
        let source_key = format!("{role}/{source}");
        let mut ancestor = snapshot.source.parent();
        while let Some(path) = ancestor {
            if !path.as_os_str().is_empty() {
                let path = normalized_relative_path(path)?;
                if entries
                    .get(&format!("{role}/{path}"))
                    .is_some_and(|value| value["type"] != "directory")
                {
                    bail!("owned OUT_DIR source crosses a non-directory ancestor");
                }
            }
            ancestor = path.parent();
        }
        crate::out_dir::validate_cas(cas, snapshot)?;
        match entries.get(&source_key) {
            None => continue,
            Some(value) if value["type"] == "directory" => {}
            Some(_) => bail!("owned OUT_DIR source is not a directory"),
        }
        let prefix = format!("{source_key}/");
        let mut directories = BTreeSet::new();
        let mut files = BTreeMap::new();
        for (path, value) in entries {
            let Some(relative) = path.strip_prefix(&prefix) else {
                continue;
            };
            let relative = PathBuf::from(relative);
            match value["type"].as_str() {
                Some("directory") => {
                    directories.insert(relative);
                }
                Some("file") => {
                    if value["content"]["kind"] != "digest" {
                        bail!("owned OUT_DIR source has no transported content identity");
                    }
                    let digest = serde_json::from_value(value["content"]["digest"].clone())?;
                    let mode = value["mode"]
                        .as_u64()
                        .ok_or_else(|| eyre::eyre!("invalid OUT_DIR file mode"))?;
                    files.insert(relative, (digest, cfg!(unix) && mode & 0o111 != 0));
                }
                _ => bail!("owned OUT_DIR source contains an unsupported entry"),
            }
        }
        if crate::out_dir::inventory_matches(snapshot, &directories, &files)? {
            crate::out_dir::validate_inventory(snapshot, &directories, &files)?;
        }
    }
    Ok(())
}

/// Hydrate supplementary immutable owner views before publishing any Cargo root.
pub(super) fn hydrate(
    config: &Config,
    cas: &LocalCas,
    state: &WorkspaceState,
    logical: &WorkspaceRoots,
    physical: &CargoBuildRoots,
    publications: &[(PathBuf, tempfile::TempDir, PathBuf)],
) -> Result<()> {
    let build = &physical.build_dir;
    let (outer, _, staged) = publications
        .iter()
        .find(|(outer, _, _)| build.starts_with(outer))
        .ok_or_else(|| eyre::eyre!("Build role has no staging root"))?;
    let staged_build = staged.join(build.strip_prefix(outer)?);
    let owner_root = crate::out_dir::resolve_root(&config.cache_dir.join(crate::out_dir::ROOT))?;
    let sources = state
        .owned_out_dirs
        .iter()
        .map(|snapshot| {
            let source = crate::out_dir::source_path(&staged_build, &snapshot.source)?;
            if source.try_exists()?
                && crate::out_dir::source_matches(&source, snapshot, &owner_root)?
            {
                Ok(Some(source))
            } else {
                crate::out_dir::validate_cas(cas, snapshot)?;
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    for (snapshot, source) in state.owned_out_dirs.iter().zip(sources) {
        if let Some(source) = source {
            crate::out_dir::hydrate(&source, snapshot, &owner_root)?;
        } else {
            crate::out_dir::hydrate_cas(cas, snapshot, &owner_root)?;
        }
        crate::out_dir::register(&owner_root, logical, snapshot)?;
    }
    Ok(())
}
