// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::{
    WorkflowPolicy,
    config::{
        RustBinaryPreviewConfig, RustBinaryReleaseArchive, RustBinaryReleaseConfig,
        RustBinaryReleaseProducer, RustBinaryReleaseProduct, VelnorConfig,
    },
};

use crate::{OrchestratorError, internal::internal};

pub(crate) fn release(config: &VelnorConfig) -> Result<RustBinaryReleaseConfig, OrchestratorError> {
    if config.workflow.policy == WorkflowPolicy::ConsumerV1 {
        return config
            .stacks
            .rust
            .as_ref()
            .map(|rust| rust.binary_release.clone())
            .ok_or_else(|| internal("preview_config_missing"));
    }
    let mut inputs = velnor_actions_contract::native_image_candidate_inputs()
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<BTreeSet<_>>();
    for path in [
        "scripts/sdk-preview-producer.sh",
        "scripts/native-image-candidate-inventory.py",
        "crates/velnor-actions-orchestrator/src/sdk_preview_configuration.rs",
        "crates/velnor-actions-orchestrator/src/sdk_preview_producer.rs",
        "crates/velnor-actions-orchestrator/src/sdk_preview_build.rs",
        "crates/velnor-actions-cli/examples/sdk_preview_producer.rs",
    ] { inputs.insert(path.to_owned()); }
    let release = RustBinaryReleaseConfig {
        version_package: "velnor-actions-cli".to_owned(),
        version_env: "SDK_PREVIEW_VERSION_OVERRIDE".to_owned(),
        producer: RustBinaryReleaseProducer {
            task: "sdk-preview:produce".to_owned(),
            inputs: inputs.into_iter().collect(),
            tools: vec!["rust".to_owned()],
            output_directory: "dist/sdk-preview".to_owned(),
            manifest: "dist/sdk-preview/package-output.json".to_owned(),
            ..RustBinaryReleaseProducer::default()
        },
        products: [
            ("velnor-actions", "velnor-actions-cli"),
            ("velnor-archive-guard", "velnor-archive-guard"),
        ]
        .into_iter()
        .map(|(id, package)| RustBinaryReleaseProduct {
            id: id.to_owned(),
            package: package.to_owned(),
            bins: vec![id.to_owned()],
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            archive: RustBinaryReleaseArchive::TarGz,
        })
        .collect(),
        preview: RustBinaryPreviewConfig {
            enabled: true,
            ..RustBinaryPreviewConfig::default()
        },
        ..RustBinaryReleaseConfig::default()
    };
    release.validate("internal-sdk-preview")?;
    Ok(release)
}
