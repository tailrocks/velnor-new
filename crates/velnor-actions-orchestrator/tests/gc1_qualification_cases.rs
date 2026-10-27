// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use crate::gc1_qualification_support::{
    ApprovalState, FixtureCacheRef, HarnessMode, HostedCommand, ObservationClosure,
    QUALIFICATION_WORKFLOW_FILE, Workload, harness_mode, invalid, require_complete_observation,
    require_dispatchable, require_fault_dispatchable, require_nonempty_workload,
    require_qualification_destructive,
};

fn must_fail<T, E>(result: &Result<T, E>, case: &str) -> Result<(), Box<dyn Error>> {
    if result.is_err() {
        return Ok(());
    }
    Err(invalid(case))
}

fn fixture_workload() -> Workload {
    Workload {
        recipe_ids: vec!["fixture-recipe".to_owned()],
        eligible_dependency_ids: vec!["fixture-dependency".to_owned()],
    }
}

fn fixture_observation() -> ObservationClosure {
    ObservationClosure {
        complete: true,
        detail: "ledger-drained local fixture observation".to_owned(),
    }
}

fn fixture_inputs() -> BTreeMap<String, String> {
    let mut inputs = BTreeMap::new();
    inputs.insert("mode".to_owned(), "both".to_owned());
    inputs
}

fn unapproved_command() -> HostedCommand {
    HostedCommand {
        workflow_file: QUALIFICATION_WORKFLOW_FILE.to_owned(),
        case_ids: vec!["C01".to_owned()],
        inputs: fixture_inputs(),
        argv: vec![
            "gh".to_owned(),
            "workflow".to_owned(),
            "run".to_owned(),
            "qualification.yml".to_owned(),
        ],
        max_runs: 1,
        fault_allowance: 1,
        approval: ApprovalState::Unapproved {
            reason: "no campaign approval recorded".to_owned(),
        },
    }
}

fn approved_command() -> HostedCommand {
    HostedCommand {
        workflow_file: QUALIFICATION_WORKFLOW_FILE.to_owned(),
        case_ids: vec!["C01".to_owned()],
        inputs: fixture_inputs(),
        argv: vec![
            "gh".to_owned(),
            "workflow".to_owned(),
            "run".to_owned(),
            "qualification.yml".to_owned(),
        ],
        max_runs: 1,
        fault_allowance: 1,
        approval: ApprovalState::Approved {
            approver: "fixture-approver".to_owned(),
            campaign_id: "fixture-campaign".to_owned(),
        },
    }
}

pub(crate) mod gc1_aux {
    use super::*;

    #[test]
    fn rejects_empty_workload() -> Result<(), Box<dyn Error>> {
        require_nonempty_workload(&fixture_workload())?;
        require_complete_observation(&fixture_observation())?;
        must_fail(
            &require_nonempty_workload(&Workload {
                recipe_ids: Vec::new(),
                eligible_dependency_ids: vec!["fixture-dependency".to_owned()],
            }),
            "empty recipe set admitted",
        )?;
        must_fail(
            &require_nonempty_workload(&Workload {
                recipe_ids: vec!["fixture-recipe".to_owned()],
                eligible_dependency_ids: Vec::new(),
            }),
            "empty dependency set admitted",
        )?;
        must_fail(
            &require_complete_observation(&ObservationClosure {
                complete: false,
                detail: "partial fixture observation".to_owned(),
            }),
            "incomplete observation admitted",
        )?;
        must_fail(
            &require_complete_observation(&ObservationClosure {
                complete: true,
                detail: String::new(),
            }),
            "observation without detail admitted",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_unapproved_fault() -> Result<(), Box<dyn Error>> {
        let approved = approved_command();
        require_fault_dispatchable(&approved)?;
        require_dispatchable(&approved)?;
        match approved.inputs.get("mode") {
            Some(mode) if mode == "both" => {}
            _ => return Err(invalid("hosted command lost its workflow inputs")),
        }
        if let ApprovalState::Unapproved { reason } = &unapproved_command().approval {
            if reason.is_empty() {
                return Err(invalid("unapproved command has no reason"));
            }
        } else {
            return Err(invalid("unapproved fixture command reads approved"));
        }
        require_qualification_destructive(
            &FixtureCacheRef {
                cache_id: 7,
                namespace: "velnor-gc1-qualification".to_owned(),
            },
            &[7],
        )?;
        must_fail(
            &require_dispatchable(&unapproved_command()),
            "unapproved hosted command admitted",
        )?;
        must_fail(
            &require_fault_dispatchable(&unapproved_command()),
            "unapproved fault run admitted",
        )?;
        let mut zero_fault = approved_command();
        zero_fault.fault_allowance = 0;
        must_fail(
            &require_fault_dispatchable(&zero_fault),
            "fault run without allowance admitted",
        )?;
        must_fail(
            &require_qualification_destructive(
                &FixtureCacheRef {
                    cache_id: 7,
                    namespace: "production".to_owned(),
                },
                &[7],
            ),
            "production cache ID admitted for destructive fixture use",
        )?;
        must_fail(
            &require_qualification_destructive(
                &FixtureCacheRef {
                    cache_id: 9,
                    namespace: "velnor-gc1-qualification".to_owned(),
                },
                &[7],
            ),
            "unlisted cache ID admitted for destructive fixture use",
        )?;
        match harness_mode() {
            HarnessMode::LocalFixture => Ok(()),
            HarnessMode::ApprovedHosted => Err(invalid(
                "unit test defaulted to hosted dispatch; local fixtures must stay the default",
            )),
        }
    }
}

#[path = "gc1_qualification_inputs.rs"]
mod gc1_qualification_inputs;

#[path = "gc1_qualification_lifecycle.rs"]
mod gc1_qualification_lifecycle;

#[expect(
    clippy::duplicate_mod,
    reason = "T59 exercises input qualification through the shared lifecycle by path include"
)]
#[path = "../src/compiler_observation.rs"]
mod compiler_observation;

#[expect(
    clippy::duplicate_mod,
    reason = "T59 exercises input qualification through the shared lifecycle by path include"
)]
#[expect(
    unreachable_pub,
    reason = "T59 exercises the frozen replay record by path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_coverage_replay.rs"]
mod cache_coverage_replay;

#[expect(
    clippy::duplicate_mod,
    reason = "T59 exercises input qualification through the shared lifecycle by path include"
)]
#[expect(
    unreachable_pub,
    reason = "T59 exercises discovery through the owning task path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_discovery.rs"]
mod cache_discovery;

#[expect(
    clippy::duplicate_mod,
    reason = "T61 exercises coverage growth through the shared intent record by path include"
)]
#[expect(
    unreachable_pub,
    reason = "T61 exercises the active intent record by path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_intent.rs"]
mod cache_intent;

#[path = "gc1_qualification_failure.rs"]
mod gc1_qualification_failure;

#[path = "gc1_qualification_paths.rs"]
mod gc1_qualification_paths;

#[path = "gc1_qualification.rs"]
mod gc1_qualification;
