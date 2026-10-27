// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script;
use velnor_actions_contract::{RoutingWorkflow, Step, StepKind};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    ProductReleasePins, RenderedFile, Schema2WorkflowRequest, VERIFY_PATH, render_schema2_workflows,
};

const IDENTITY_SOURCE: &str = include_str!("../src/job_image_authority_identity.py");
const REGISTRY_SOURCE: &str = include_str!("../src/job_image_authority_registry.py");
const PUBLISH_SOURCE: &str = include_str!("../src/job_image_authority_publish.py");
const RECORD_SOURCE: &str = include_str!("../src/job_image_authority_record.py");
const OCI_SOURCE: &str = include_str!("../src/job_image_authority_oci.py");
const TAR_SOURCE: &str = include_str!("../src/job_image_authority_tar.py");
const AUTHORITY_MODULE: &str = concat!(
    include_str!("../src/job_image_authority.rs"),
    include_str!("../src/job_image_authority_verify.rs"),
);

const IDENTITY_FILE: &str = "job_image_authority_identity.py";
const REGISTRY_FILE: &str = "job_image_authority_registry.py";
const PUBLISH_FILE: &str = "job_image_authority_publish.py";
const RECORD_FILE: &str = "job_image_authority_record.py";
const OCI_FILE: &str = "job_image_authority_oci.py";
const TAR_FILE: &str = "job_image_authority_tar.py";

const STEP_NAME: &str = "Materialize fixed image authority implementation";
const BODY_DIRECTORY: &str = "$RUNNER_TEMP/velnor-image-authority";
const END_MARKER: &str = "VELNOR_IMAGE_AUTHORITY_EOF";
const CANDIDATE_PATH: &str = ".github/workflows/job-image-candidate.yml";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn require_count(
    haystack: &str,
    needle: &str,
    expected: usize,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual = haystack.matches(needle).count();
    if actual == expected {
        Ok(())
    } else {
        Err(fail(&format!(
            "{label} left:{actual}:expected:{expected}:{needle}"
        )))
    }
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

#[path = "gc1_t47_archive.rs"]
mod gc1_t47_archive;
#[path = "gc1_t47_argv.rs"]
mod gc1_t47_argv;
#[path = "gc1_t47_driver.rs"]
mod gc1_t47_driver;
#[path = "gc1_t47_inputs.rs"]
mod gc1_t47_inputs;
#[path = "gc1_t47_reject.rs"]
mod gc1_t47_reject;
#[path = "gc1_t47_shared.rs"]
mod gc1_t47_shared;
#[path = "gc1_t47_verify.rs"]
mod gc1_t47_verify;
pub(crate) use gc1_t47_archive::*;
pub(crate) use gc1_t47_argv::*;
pub(crate) use gc1_t47_driver::*;
pub(crate) use gc1_t47_inputs::*;
pub(crate) use gc1_t47_shared::*;
