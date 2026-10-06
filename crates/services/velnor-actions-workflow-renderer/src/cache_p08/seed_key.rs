use crate::RenderError;

/// Tools-cache key derived from the complete pinned job payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MiseToolsCacheKey(String);

impl MiseToolsCacheKey {
    pub(crate) fn derive(
        target: &str,
        mise_version: &str,
        specs: &[String],
    ) -> Result<Self, RenderError> {
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
            "{}-{target}-{mise_version}-{}",
            super::MISE_KEY_PREFIX,
            super::tools_digest(specs)
        )))
    }

    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
