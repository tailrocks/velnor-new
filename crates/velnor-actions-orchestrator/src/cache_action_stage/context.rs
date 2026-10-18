use super::StageError;
use std::path::{Component, Path, PathBuf};

const STAGE_DOMAIN: &[u8] = b"velnor-cache-action-stage-v1\0";

/// Holds independently validated cache scope and stage paths.
#[derive(Debug, Clone)]
pub struct StageContext {
    runner_temp: PathBuf,
    workspace: PathBuf,
    action_path: PathBuf,
    member_path: String,
}

impl StageContext {
    /// Creates a context for one exact normal-tools key.
    ///
    /// # Errors
    ///
    /// Returns an error when a path or key is invalid or a root cannot be resolved.
    pub fn normal(runner_temp: &Path, workspace: &Path, key: &str) -> Result<Self, StageError> {
        let identity = key
            .strip_prefix("mise-tools-v2-payload-v3-")
            .ok_or("cache_stage_context_invalid")?;
        if identity.len() != 64
            || !identity
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("cache_stage_context_invalid");
        }
        validate_key(key)?;
        Self::new(runner_temp, workspace, "normal-tools-v3", &[], key)
    }

    /// Creates a context for one exact qualification key.
    ///
    /// # Errors
    ///
    /// Returns an error when the scope, key or path is invalid.
    pub fn qualification(
        runner_temp: &Path,
        workspace: &Path,
        campaign: &str,
        layer: &str,
        key: &str,
    ) -> Result<Self, StageError> {
        validate_scope(campaign, layer, key)?;
        Self::new(
            runner_temp,
            workspace,
            "qualification-v3",
            &[campaign, layer],
            key,
        )
    }

    fn new(
        runner_temp: &Path,
        workspace: &Path,
        namespace: &str,
        scope: &[&str],
        key: &str,
    ) -> Result<Self, StageError> {
        let runner_temp = clean_absolute(runner_temp)?;
        let workspace = clean_absolute(workspace)?;
        let mut fields = Vec::with_capacity(scope.len() + 2);
        fields.push(namespace);
        fields.extend_from_slice(scope);
        fields.push(key);
        let mut hash = blake3::Hasher::new();
        hash.update(STAGE_DOMAIN);
        for field in &fields {
            let length = u64::try_from(field.len()).map_err(|_| "cache_stage_context_invalid")?;
            hash.update(&length.to_be_bytes());
            hash.update(field.as_bytes());
        }
        let digest = hash.finalize();
        let leaf = format!("velnor-cache-transport-v1-{}.payload", digest.to_hex());
        let action_path = runner_temp.join(leaf);
        let member_path = relative_path(&workspace, &action_path)?;
        Ok(Self {
            runner_temp,
            workspace,
            action_path,
            member_path,
        })
    }

    /// Gives the exact action input path for this cache context.
    #[must_use]
    pub fn action_path(&self) -> &Path {
        &self.action_path
    }

    pub(crate) fn runner_temp(&self) -> &Path {
        &self.runner_temp
    }

    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) fn member_path(&self) -> &str {
        &self.member_path
    }
}

fn clean_absolute(path: &Path) -> Result<PathBuf, StageError> {
    if !path.is_absolute() || path.as_os_str().len() > 4_096 {
        return Err("cache_stage_path_invalid");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push("/"),
            Component::Normal(value) => normalized.push(value),
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("cache_stage_path_invalid");
            }
        }
    }
    if normalized == Path::new("/") {
        return Err("cache_stage_path_invalid");
    }
    let canonical = std::fs::canonicalize(&normalized).map_err(|_| "cache_stage_path_invalid")?;
    if canonical.as_os_str().len() > 4_096 {
        return Err("cache_stage_path_invalid");
    }
    Ok(canonical)
}

fn validate_key(key: &str) -> Result<(), StageError> {
    if key.is_empty()
        || key.len() > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
        || key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("cache_stage_context_invalid");
    }
    Ok(())
}

fn validate_scope(campaign: &str, layer: &str, key: &str) -> Result<(), StageError> {
    let valid_campaign = !campaign.is_empty()
        && campaign.len() <= 64
        && campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let valid_layer = matches!(
        layer,
        "mbx_objects" | "cargo_sources" | "mise_tools" | "tofu_providers" | "task_result"
    );
    let expected_prefix = format!(
        "velnor-qcache-v{}-{campaign}-{layer}-",
        velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION
    );
    let suffix = key.strip_prefix(&expected_prefix).unwrap_or_default();
    if !valid_campaign
        || !valid_layer
        || suffix.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        return Err("cache_stage_context_invalid");
    }
    validate_key(key)?;
    Ok(())
}

fn relative_path(base: &Path, target: &Path) -> Result<String, StageError> {
    let base: Vec<_> = base.components().collect();
    let target: Vec<_> = target.components().collect();
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec![".."; base.len().saturating_sub(common)];
    for component in &target[common..] {
        let Component::Normal(value) = component else {
            return Err("cache_stage_path_invalid");
        };
        parts.push(value.to_str().ok_or("cache_stage_path_invalid")?);
    }
    let text = parts.join("/");
    if text.is_empty() || text.len() > 4_096 || text.starts_with('/') {
        return Err("cache_stage_path_invalid");
    }
    Ok(text)
}
