use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    PackageReleasePlan, package_release::PackageReleaseChannel, preview_identity::PreviewIdentity,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseTaskIdentity {
    pub channel: PackageReleaseChannel,
    pub base_version: String,
    pub version: String,
    pub tag: String,
    pub source_sha: String,
    pub commit_count: u64,
}
impl ReleaseTaskIdentity {
    pub(super) fn derive(
        plan: &PackageReleasePlan,
        sha: &str,
        count: u64,
    ) -> Result<Self, OrchestratorError> {
        let (version, tag) = match plan.channel {
            PackageReleaseChannel::Preview => {
                let preview = PreviewIdentity::derive(&plan.base_version, sha, count)
                    .map_err(|_| internal("release_task_identity"))?;
                (preview.version, preview.tag)
            }
            PackageReleaseChannel::Stable => {
                (plan.base_version.clone(), format!("v{}", plan.base_version))
            }
        };
        Ok(Self {
            channel: plan.channel,
            base_version: plan.base_version.clone(),
            version,
            tag,
            source_sha: sha.to_owned(),
            commit_count: count,
        })
    }
    pub(super) fn canonical(&self) -> bool {
        match self.channel {
            PackageReleaseChannel::Preview => {
                PreviewIdentity::derive(&self.base_version, &self.source_sha, self.commit_count)
                    .is_ok_and(|preview| self.version == preview.version && self.tag == preview.tag)
            }
            PackageReleaseChannel::Stable => {
                self.commit_count == 0
                    && velnor_actions_contract::config::is_exact_semver(&self.base_version)
                    && self.source_sha.len() == 40
                    && self
                        .source_sha
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    && self.version == self.base_version
                    && self.tag == format!("v{}", self.base_version)
            }
        }
    }
}
