// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use serde::Serialize;
use std::collections::BTreeMap;
use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::{
    JOB_IMAGE_CANDIDATE_DRIVER, JobImageCandidateInputs, ProposedTask, job_image_candidate_proposal,
};
use velnor_actions_mise::{PinnedTool, PrepareRustComponents, ToolCatalog};

#[derive(Debug, Clone, Serialize)]
pub struct JobImageCandidateExecution {
    inputs: JobImageCandidateInputs,
    argv: Vec<String>,
    environment: BTreeMap<String, String>,
    binding_toolchain: ToolchainInputs,
    actual_toolchain: ToolchainInputs,
}

impl JobImageCandidateExecution {
    pub fn for_catalog(catalog: &ToolCatalog) -> Result<Self, RenderError> {
        if catalog.is_emission() {
            return Err(RenderError::InvalidWorkflow(
                "candidate_catalog_must_be_concrete".to_owned(),
            ));
        }
        let emission = catalog.for_emission();
        let inputs = JobImageCandidateInputs {
            rust: emission.tool_spec(PinnedTool::Rust),
            mr_boxington: emission.tool_spec(PinnedTool::MrBoxington),
            export_rustup: emission.exports_rustup_toolchain(),
        };
        Ok(Self {
            argv: inputs.argv().map_err(RenderError::Contract)?,
            environment: inputs.environment().map_err(RenderError::Contract)?,
            inputs,
            binding_toolchain: toolchain(&emission),
            actual_toolchain: toolchain(catalog),
        })
    }
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
    #[must_use]
    pub fn binding_toolchain(&self) -> &ToolchainInputs {
        &self.binding_toolchain
    }
    #[must_use]
    pub fn actual_toolchain(&self) -> &ToolchainInputs {
        &self.actual_toolchain
    }
    #[must_use]
    pub fn inputs(&self) -> &JobImageCandidateInputs {
        &self.inputs
    }
    pub fn proposal(&self) -> Result<ProposedTask, RenderError> {
        job_image_candidate_proposal(&self.inputs).map_err(RenderError::Contract)
    }
}

fn toolchain(catalog: &ToolCatalog) -> ToolchainInputs {
    let mut tools = catalog.tool_specs(&[PinnedTool::Rust, PinnedTool::MrBoxington]);
    tools.sort();
    ToolchainInputs {
        tools,
        components: PrepareRustComponents::components(),
        compile_driver: JOB_IMAGE_CANDIDATE_DRIVER.to_owned(),
        test_runner: JOB_IMAGE_CANDIDATE_DRIVER.to_owned(),
    }
}
