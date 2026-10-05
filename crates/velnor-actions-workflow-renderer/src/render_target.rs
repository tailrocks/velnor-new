//! Resolve the exact cache target from an already-validated runner label.

use velnor_actions_contract::{ReleaseTarget, RunsOn, SCALE_SET_NAME};

pub(super) fn target_for_runner(label: &str) -> Option<&'static str> {
    match RunsOn::parse(label).ok()? {
        RunsOn::Hosted(label) => ReleaseTarget::for_runner_label(&label).map(ReleaseTarget::triple),
        RunsOn::ScaleSet(selector) if selector.name() == SCALE_SET_NAME => {
            Some(ReleaseTarget::LinuxX86_64.triple())
        }
        RunsOn::ScaleSet(_) => None,
    }
}
