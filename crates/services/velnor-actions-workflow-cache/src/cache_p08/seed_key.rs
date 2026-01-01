use velnor_actions_workflow_steps::RenderError;

/// Tools-cache key derived from the complete pinned job payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MiseToolsCacheKey(String);

impl MiseToolsCacheKey {
    pub(crate) fn derive(
        image_os: &str,
        target: &str,
        mise_version: &str,
        specs: &[String],
    ) -> Result<Self, RenderError> {
        let image_target_ok = match image_os {
            "ubuntu22" | "ubuntu24" | "ubuntu26" => target == "x86_64-unknown-linux-gnu",
            "macos15" => matches!(target, "aarch64-apple-darwin" | "x86_64-apple-darwin"),
            _ => false,
        };
        if !image_target_ok {
            return Err(RenderError::BadCommand(format!(
                "bad_cache_image_target:{image_os}:{target}"
            )));
        }
        if !velnor_actions_contract_release::is_supported_target(target) {
            return Err(RenderError::BadCommand(format!(
                "bad_cache_target:{target}"
            )));
        }
        if !super::is_catalog_version(mise_version) {
            return Err(RenderError::BadCommand(format!(
                "bad_mise_version:{mise_version}"
            )));
        }
        if specs.is_empty() {
            return Err(RenderError::BadCommand("empty_tool_specs".to_owned()));
        }
        for spec in specs {
            if !super::is_tool_spec(spec) {
                return Err(RenderError::BadCommand(format!("bad_tool_spec:{spec}")));
            }
        }
        Ok(Self(format!(
            "{}-{image_os}-{target}-{mise_version}-{}-{}",
            super::MISE_KEY_PREFIX,
            super::tools_digest(specs),
            super::MISE_CACHE_SUFFIX_EXPR
        )))
    }

    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
