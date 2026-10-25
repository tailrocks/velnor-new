use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, RunnerPlatform, RunsOn, runner_platform};

use super::{RenderError, invalid};

#[derive(Clone, Debug)]
pub(crate) struct CaptureRunnerMetadata {
    #[cfg(feature = "test-render-capture")]
    pub(crate) runner_class: &'static str,
    #[cfg(feature = "test-render-capture")]
    pub(crate) configured_runner: String,
    #[cfg(feature = "test-render-capture")]
    pub(crate) final_runner_labels: Vec<String>,
}

pub(crate) fn capture_runner_metadata(
    source_id: &str,
    source: &Job,
) -> Result<CaptureRunnerMetadata, RenderError> {
    if source.check_runner.is_some() || !source.services.is_empty() {
        return Err(invalid(
            source_id,
            "typed_rust_job_has_native_or_service_metadata",
        ));
    }
    let runner = RunsOn::parse(&source.runs_on)
        .map_err(|_| invalid(source_id, "typed_rust_runner_selector_invalid"))?;
    if source_id.ends_with(HOSTED_SUFFIX) {
        let RunsOn::Hosted(label) = runner else {
            return Err(invalid(source_id, "hosted_lane_is_not_hosted_runner"));
        };
        if runner_platform(&label) != Some(RunnerPlatform::LinuxAmd64) {
            return Err(invalid(source_id, "hosted_lane_is_not_linux_x64"));
        }
        Ok(CaptureRunnerMetadata {
            #[cfg(feature = "test-render-capture")]
            runner_class: "hosted-linux-x64",
            #[cfg(feature = "test-render-capture")]
            configured_runner: source.runs_on.clone(),
            #[cfg(feature = "test-render-capture")]
            final_runner_labels: vec![label],
        })
    } else if source_id.ends_with(SCALE_SUFFIX) {
        let RunsOn::ScaleSet(selector) = runner else {
            return Err(invalid(source_id, "scale_lane_is_not_configured_scale_set"));
        };
        #[cfg(not(feature = "test-render-capture"))]
        let _labels = selector.labels();
        Ok(CaptureRunnerMetadata {
            #[cfg(feature = "test-render-capture")]
            runner_class: "configured-scale-set-linux-x64",
            #[cfg(feature = "test-render-capture")]
            configured_runner: source.runs_on.clone(),
            #[cfg(feature = "test-render-capture")]
            final_runner_labels: selector.labels().to_vec(),
        })
    } else {
        Err(invalid(source_id, "typed_rust_runner_lane_suffix_missing"))
    }
}
