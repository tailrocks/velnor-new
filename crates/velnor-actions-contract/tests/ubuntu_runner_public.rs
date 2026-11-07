// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_contract::{ReleaseTarget, config::WorkflowConfig};

#[test]
fn workflow_configuration_accepts_only_pinned_ubuntu_26_labels() -> Result<(), Box<dyn Error>> {
    for label in ["ubuntu-26.04", "ubuntu-26.04-arm"] {
        configured(label)?.validate("config.toml")?;
    }
    for label in [
        "ubuntu-22.04",
        "ubuntu-22.04-arm",
        "ubuntu-24.04",
        "ubuntu-24.04-arm",
        "ubuntu-latest",
    ] {
        let error = configured(label)?
            .validate("config.toml")
            .err()
            .ok_or("accepted unsupported Ubuntu runner")?;
        assert!(error.to_string().contains("workflow.runner_label"));
        assert!(error.to_string().contains("unsupported_label"));
    }
    Ok(())
}

#[test]
fn sdk_linux_release_target_requires_pinned_ubuntu_26() {
    assert_eq!(
        ReleaseTarget::for_runner_label("ubuntu-26.04"),
        Some(ReleaseTarget::LinuxX86_64)
    );
    for label in ["ubuntu-22.04", "ubuntu-24.04", "ubuntu-latest"] {
        assert_eq!(ReleaseTarget::for_runner_label(label), None);
    }
}

fn configured(label: &str) -> Result<WorkflowConfig, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "name": "CI", "policy": "consumer-v1", "generator_validation": "bootstrap",
        "max_parallel_jobs": 2, "runner_label": label,
    }))
}
