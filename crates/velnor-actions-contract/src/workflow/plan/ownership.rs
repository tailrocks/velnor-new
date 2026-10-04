//! Bind every matrix task to its persisted obligation owner.
use super::{ExecuteTaskRef, PlanMatrix, PlanObligation};
use crate::errors::ContractError;
use crate::workflow::jobs::{is_crate_job_id, validate_job_id};

pub(super) fn validate_obligation_job(job_id: &str) -> Result<(), ContractError> {
    validate_job_id(job_id)?;
    if job_id != "plan" && !is_crate_job_id(job_id) {
        return Err(ContractError::identity(
            "obligations.job_id",
            "unknown_job_owner",
        ));
    }
    Ok(())
}

pub(super) fn check_matrix_ownership(
    matrix: &PlanMatrix,
    obligations: &[PlanObligation],
) -> Result<(), ContractError> {
    for entry in &matrix.include {
        if let Ok(index) = obligations.binary_search_by(|ob| ob.task_id.cmp(&entry.task_id)) {
            check_owner(&obligations[index], &entry.job_id)?;
        }
        for task_ref in entry.execute_task_ids.tasks.values().chain(&entry.test_run) {
            let ids = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(ids) => ids.as_slice(),
            };
            for id in ids {
                let index = obligations
                    .binary_search_by(|ob| ob.task_id.cmp(id))
                    .map_err(|_| {
                        ContractError::identity(
                            "matrix.include.execute_task_ids",
                            "unknown_obligation",
                        )
                    })?;
                check_owner(&obligations[index], &entry.job_id)?;
            }
        }
    }
    Ok(())
}

fn check_owner(obligation: &PlanObligation, job_id: &str) -> Result<(), ContractError> {
    if obligation.job_id != job_id {
        return Err(ContractError::identity(
            "matrix.include.job_id",
            "obligation_owner_mismatch",
        ));
    }
    Ok(())
}

/// Check `task_ids` contains every obligation ID, nothing else (wf §4).
pub(super) fn check_obligation_agreement(
    task_ids: &[String],
    obligations: &[PlanObligation],
) -> Result<(), ContractError> {
    let mut expected: Vec<&str> = obligations
        .iter()
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    expected.sort_unstable();
    let mut actual: Vec<&str> = task_ids.iter().map(String::as_str).collect();
    actual.sort_unstable();
    if expected == actual {
        Ok(())
    } else {
        Err(ContractError::identity("task_ids", "obligation_mismatch"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::digest_b3;
    use crate::workflow::execute::ExecuteTaskIds;
    use crate::workflow::plan::{MatrixEntry, ObligationDecision};
    use std::collections::BTreeMap;

    const TASK: &str = "stack/rust/demo/clippy/default";
    const SHARD: &str = "stack/rust/demo/nextest/default/shard-1-of-2";

    fn obligation(task_id: &str, job_id: &str) -> PlanObligation {
        let digest = digest_b3(b"fixture");
        PlanObligation {
            task_id: task_id.to_owned(),
            job_id: job_id.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: digest.clone(),
            input_digest: digest.clone(),
            execution_identity: crate::TaskExecutionIdentity::new(
                &crate::digest_b3(b"fixture-graph"),
                &crate::digest_b3(b"fixture-toolchain"),
                &crate::digest_b3(b"fixture-mbx"),
                &crate::digest_b3(b"fixture-platform"),
                "default",
            )
            .expect("execution identity"),
            closure_digest: digest,
            baseline_proof: None,
        }
    }

    fn matrix(task_ref: ExecuteTaskRef) -> PlanMatrix {
        let digest = digest_b3(b"fixture");
        let entry = MatrixEntry::derive(
            "rust",
            "stack/rust/demo/validation/default",
            "true",
            &digest,
            serde_json::json!({}),
            ExecuteTaskIds {
                tasks: BTreeMap::from([("clippy".to_owned(), task_ref)]),
            },
            &digest,
            "local",
            "rust-demo",
        )
        .expect("fixture entry");
        PlanMatrix {
            include: vec![entry],
        }
    }

    #[test]
    fn owner_is_required_when_deserializing() {
        let mut json = serde_json::to_value(obligation(TASK, "rust-demo")).expect("serialize");
        json.as_object_mut().expect("object").remove("job_id");
        assert!(serde_json::from_value::<PlanObligation>(json).is_err());
    }

    #[test]
    fn obligation_owner_requires_crate_or_plan_job() {
        for valid in ["plan", "rust-demo", "tofu-infra"] {
            assert!(validate_obligation_job(valid).is_ok(), "{valid}");
        }
        for invalid in ["", "required", "actionlint", "velnor-plan", "rust-Demo"] {
            assert!(validate_obligation_job(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn matrix_owner_must_match_obligation_owner() {
        let matrix = matrix(ExecuteTaskRef::Single(TASK.to_owned()));
        assert!(check_matrix_ownership(&matrix, &[obligation(TASK, "rust-demo")]).is_ok());
        assert!(check_matrix_ownership(&matrix, &[obligation(TASK, "rust-other")]).is_err());
        assert!(check_matrix_ownership(&matrix, &[]).is_err());
    }

    #[test]
    fn every_shard_and_test_run_ref_is_bound_to_owner() {
        let mut matrix = matrix(ExecuteTaskRef::Shards(vec![
            TASK.to_owned(),
            SHARD.to_owned(),
        ]));
        let obligations = [
            obligation(TASK, "rust-demo"),
            obligation(SHARD, "rust-demo"),
        ];
        assert!(check_matrix_ownership(&matrix, &obligations).is_ok());
        assert!(check_matrix_ownership(&matrix, &obligations[..1]).is_err());
        matrix.include[0]
            .execute_task_ids
            .tasks
            .insert("clippy".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()));
        matrix.include[0].test_run = vec![ExecuteTaskRef::Single(SHARD.to_owned())];
        let bad = [
            obligation(TASK, "rust-demo"),
            obligation(SHARD, "rust-other"),
        ];
        assert!(check_matrix_ownership(&matrix, &bad).is_err());
    }
}
