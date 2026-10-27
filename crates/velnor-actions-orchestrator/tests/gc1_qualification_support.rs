// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::io;

pub(crate) const QUALIFICATION_NAMESPACE: &str = "velnor-gc1-qualification";
pub(crate) const QUALIFICATION_WORKFLOW_FILE: &str = ".github/workflows/qualification.yml";
pub(crate) const HOSTED_OPT_IN_VAR: &str = "VELNOR_GC1_QUALIFICATION_HOSTED";

pub(crate) const QUALIFICATION_CASE_IDS: [&str; 22] = [
    "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11", "C12", "C13",
    "C14", "C15", "C16", "C17", "C18", "C19", "C20", "C21", "C22",
];

pub(crate) fn invalid(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

pub(crate) enum HarnessMode {
    LocalFixture,
    ApprovedHosted,
}

pub(crate) fn harness_mode() -> HarnessMode {
    match env::var(HOSTED_OPT_IN_VAR) {
        Ok(value) if value == "1" => HarnessMode::ApprovedHosted,
        _ => HarnessMode::LocalFixture,
    }
}

pub(crate) struct Workload {
    pub recipe_ids: Vec<String>,
    pub eligible_dependency_ids: Vec<String>,
}

pub(crate) fn require_nonempty_workload(workload: &Workload) -> Result<(), Box<dyn Error>> {
    if workload.recipe_ids.is_empty() {
        return Err(invalid("qualification workload has no recipes"));
    }
    if workload.eligible_dependency_ids.is_empty() {
        return Err(invalid(
            "qualification workload has no eligible dependencies",
        ));
    }
    Ok(())
}

pub(crate) struct ObservationClosure {
    pub complete: bool,
    pub detail: String,
}

pub(crate) fn require_complete_observation(
    observation: &ObservationClosure,
) -> Result<(), Box<dyn Error>> {
    if !observation.complete {
        return Err(invalid("qualification observation is incomplete"));
    }
    if observation.detail.is_empty() {
        return Err(invalid("qualification observation has no detail"));
    }
    Ok(())
}

pub(crate) enum ApprovalState {
    Unapproved {
        reason: String,
    },
    Approved {
        approver: String,
        campaign_id: String,
    },
}

pub(crate) fn require_approved(approval: &ApprovalState) -> Result<(), Box<dyn Error>> {
    match approval {
        ApprovalState::Approved {
            approver,
            campaign_id,
        } => {
            if approver.is_empty() {
                return Err(invalid("hosted approval has no approver"));
            }
            if campaign_id.is_empty() {
                return Err(invalid("hosted approval has no campaign"));
            }
            Ok(())
        }
        ApprovalState::Unapproved { .. } => Err(invalid("hosted command is not approved")),
    }
}

pub(crate) struct HostedCommand {
    pub workflow_file: String,
    pub case_ids: Vec<String>,
    pub inputs: BTreeMap<String, String>,
    pub argv: Vec<String>,
    pub max_runs: u64,
    pub fault_allowance: u64,
    pub approval: ApprovalState,
}

pub(crate) fn require_known_case_ids(case_ids: &[String]) -> Result<(), Box<dyn Error>> {
    if case_ids.is_empty() {
        return Err(invalid("hosted command binds no case IDs"));
    }
    let mut seen = BTreeSet::new();
    for case_id in case_ids {
        if !QUALIFICATION_CASE_IDS.contains(&case_id.as_str()) {
            return Err(invalid("hosted command binds an unknown case ID"));
        }
        if !seen.insert(case_id.clone()) {
            return Err(invalid("hosted command binds a duplicate case ID"));
        }
    }
    Ok(())
}

pub(crate) fn require_dispatchable(command: &HostedCommand) -> Result<(), Box<dyn Error>> {
    require_approved(&command.approval)?;
    if command.workflow_file.is_empty() {
        return Err(invalid("hosted command has no workflow file"));
    }
    if command.argv.is_empty() {
        return Err(invalid("hosted command has no argv"));
    }
    if command.max_runs == 0 {
        return Err(invalid("hosted command allows no runs"));
    }
    require_known_case_ids(&command.case_ids)?;
    Ok(())
}

pub(crate) fn require_fault_dispatchable(command: &HostedCommand) -> Result<(), Box<dyn Error>> {
    require_dispatchable(command)?;
    if command.fault_allowance == 0 {
        return Err(invalid("fault command has no fault allowance"));
    }
    Ok(())
}

pub(crate) struct FixtureCacheRef {
    pub cache_id: u64,
    pub namespace: String,
}

pub(crate) fn require_qualification_destructive(
    target: &FixtureCacheRef,
    approved_ids: &[u64],
) -> Result<(), Box<dyn Error>> {
    if target.namespace != QUALIFICATION_NAMESPACE {
        return Err(invalid(
            "destructive fixture operation outside the qualification namespace",
        ));
    }
    if !approved_ids.contains(&target.cache_id) {
        return Err(invalid(
            "destructive fixture operation on an unapproved cache ID",
        ));
    }
    Ok(())
}
