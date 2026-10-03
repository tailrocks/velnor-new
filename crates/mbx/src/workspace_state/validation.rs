use super::*;

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
            Some(entries) => {
                if entries.get("target") != Some(&serde_json::json!({"type": "root"})) {
                    bail!("workspace semantic inventory has no target role marker");
                }
                for path in entries.keys() {
                    let role = path
                        .split('/')
                        .next()
                        .ok_or_else(|| eyre::eyre!("workspace entry has no role"))?;
                    if entries.get(role) != Some(&serde_json::json!({"type": "root"})) {
                        bail!("workspace semantic entry has no role marker");
                    }
                }
                serde_json::to_vec(entries)?
            }
            None => bail!("workspace semantic inventory has no root roles"),
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
        let (role, relative) = path.split_once('/').unwrap_or((&path, ""));
        if !matches!(role, "target" | "build" | "out_dir") {
            bail!("workspace semantic entry has invalid role");
        }
        if !relative.is_empty() {
            validate_relative_path(Path::new(relative))?;
        }
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
    if key.path.as_deref().is_some_and(|path| !path.contains('/')) && kind != "root" {
        bail!("workspace semantic role marker has an invalid type");
    }
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
        (Some("target" | "build" | "out_dir"), "root") => {
            require_semantic_fields(object, &["type"])?;
            Ok(None)
        }
        (Some(path), "owned_out_dir") if path.starts_with("out_dir/") => {
            require_semantic_fields(object, &["type", "content"])?;
            validate_semantic_content(object.get("content"), false)?;
            Ok(None)
        }
        (Some(path), "file") if !path.starts_with("out_dir/") => {
            require_semantic_fields(object, &["type", "content", "mode"])?;
            validate_semantic_mode(object)?;
            validate_semantic_content(object.get("content"), true)?;
            Ok(None)
        }
        (Some(path), "directory") if !path.starts_with("out_dir/") => {
            require_semantic_fields(object, &["type", "mode"])?;
            validate_semantic_mode(object)?;
            Ok(None)
        }
        (Some(path), "symlink") if !path.starts_with("out_dir/") => {
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
            let (_, relative) = path
                .split_once('/')
                .ok_or_else(|| eyre::eyre!("workspace semantic entry has no role path"))?;
            validate_link(Path::new(relative), target_path)?;
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
