use crate::{ContractError, config::is_exact_semver};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewIdentity {
    pub version: String,

    pub tag: String,

    pub source_sha: String,

    pub commit_count: u64,
}

impl PreviewIdentity {
    pub fn derive(base: &str, source_sha: &str, commit_count: u64) -> Result<Self, ContractError> {
        if !is_exact_semver(base) {
            return Err(ContractError::config(
                "release",
                "base_version",
                "preview_requires_numeric_base_version",
            ));
        }
        if source_sha.len() != 40
            || !source_sha
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(ContractError::config(
                "release",
                "source_sha",
                "requires_full_lowercase_commit_sha",
            ));
        }
        if commit_count == 0 {
            return Err(ContractError::config(
                "release",
                "commit_count",
                "requires_full_history_count",
            ));
        }
        let version = format!("{base}-preview.{commit_count}+{}", &source_sha[..7]);
        Ok(Self {
            tag: format!("preview-{source_sha}"),
            version,
            source_sha: source_sha.to_owned(),
            commit_count,
        })
    }
}
