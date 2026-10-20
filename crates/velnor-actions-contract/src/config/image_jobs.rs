//! Lookup from a generated image build job id back to its declaration.

use super::{ImageConfig, ImagePlatform, VelnorConfig};
use crate::errors::ContractError;

impl VelnorConfig {
    /// The declared image and platform whose build job has this id.
    #[must_use]
    pub fn image_for_build_job(&self, job_id: &str) -> Option<(&ImageConfig, ImagePlatform)> {
        self.images.iter().find_map(|image| {
            image
                .platforms
                .iter()
                .find(|platform| image.build_job_id(**platform) == job_id)
                .map(|platform| (image, *platform))
        })
    }

    /// For an image build job id: whether `runs_on` is its native runner.
    ///
    /// `None` when the id is not an image build job.
    pub(crate) fn check_image_job_runner(
        &self,
        job_id: &str,
        runs_on: &str,
    ) -> Option<Result<(), ContractError>> {
        let (_, platform) = self.image_for_build_job(job_id)?;
        let expected = platform.runs_on();
        Some(if runs_on == expected {
            Ok(())
        } else {
            Err(ContractError::config(
                "config.toml",
                format!("images.runner:{job_id}"),
                format!("runner_mismatch:{expected}:{runs_on}"),
            ))
        })
    }
}
