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
use velnor_actions_workflow_renderer::VERIFY_PATH;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::steps::{ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES};
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    ProductReleasePins, RenderedFile, Schema2WorkflowRequest, render_schema2_workflows,
};

const CANDIDATE_PATH: &str = ".github/workflows/job-image-candidate.yml";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

#[path = "gc1_t48_archive.rs"]
mod gc1_t48_archive;
#[path = "gc1_t48_argv.rs"]
mod gc1_t48_argv;
#[path = "gc1_t48_checks.rs"]
mod gc1_t48_checks;
#[path = "gc1_t48_pins.rs"]
mod gc1_t48_pins;
use gc1_t48_archive::*;
use gc1_t48_argv::*;
use gc1_t48_checks::*;
use gc1_t48_pins::*;
