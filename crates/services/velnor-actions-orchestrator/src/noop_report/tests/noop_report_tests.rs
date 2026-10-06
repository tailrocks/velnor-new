use super::*;

#[test]
fn noop_report_writes_not_selected_with_reason() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged_run(&plan);
    let entry = &plan.matrix.include[0];
    let request = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest: task_digest.clone(),
    };
    let reported = write_noop_report_to("local", CLIPPY, 0, &request, temp.path()).expect("report");
    assert_eq!(reported, 1);
    let expect_id =
        velnor_actions_contract::task_report_id_for_task("local", &entry.matrix_key, &task_digest)
            .expect("task report id");
    let (task, matrix) = read_entry(&temp, &entry.matrix_key, &expect_id);
    assert_eq!(task.status, TaskStatus::NotSelected);
    assert_eq!(
        task.not_selected_reason,
        Some(NotSelectedReason::Unsupported)
    );
    assert_eq!(task.exit_code, 0);
    assert_eq!(task.task_digest, task_digest);
    assert_eq!(matrix.status, MatrixStatus::Passed);
    assert_eq!(matrix.not_selected, 1);
    assert_eq!(matrix.report_id, entry.report_id);
}

#[test]
fn noop_request_parsing_is_strict() {
    for (word, reason) in [
        ("upstream_failed", NotSelectedReason::UpstreamFailed),
        ("not_in_plan", NotSelectedReason::NotInPlan),
        ("unsupported", NotSelectedReason::Unsupported),
        ("cancelled_by_policy", NotSelectedReason::CancelledByPolicy),
    ] {
        assert_eq!(parse_not_selected_reason(word).expect("reason"), reason);
    }
    for bad in ["", "success", "Unsupported", "not_selected", "skipped"] {
        assert!(parse_not_selected_reason(bad).is_err(), "{bad:?}");
    }
    assert!(parse_noop_request(None, None).expect("absent").is_none());
    let good_digest = digest(1);
    assert!(
        parse_noop_request(Some("unsupported"), Some(&good_digest))
            .expect("pair")
            .is_some()
    );
    assert!(parse_noop_request(Some("unsupported"), None).is_err());
    assert!(parse_noop_request(None, Some(&good_digest)).is_err());
    assert!(parse_noop_request(Some("bogus"), Some(&good_digest)).is_err());
    assert!(parse_noop_request(Some("unsupported"), Some("b3-short")).is_err());
}

#[test]
fn noop_rejects_contradictions() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged_run(&plan);
    let request = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest: task_digest.clone(),
    };
    let err = write_noop_report_to("local", CLIPPY, 1, &request, temp.path()).expect_err("exit");
    assert!(err.to_string().contains("reason_with_failure"), "{err}");
    let drifted = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest: digest(9),
    };
    let err = write_noop_report_to("local", CLIPPY, 0, &drifted, temp.path()).expect_err("digest");
    assert!(err.to_string().contains("noop_digest_mismatch"), "{err}");
    assert!(
        write_noop_report_to(
            "local",
            "stack/rust/demo/test/default",
            0,
            &request,
            temp.path()
        )
        .is_err()
    );
}

#[test]
fn noop_op_contract_pins_wire_strings() {
    assert_eq!(REPORT_OP, "write-task-report-v1");
    assert_eq!(NOT_SELECTED_REASON_ENV, "VELNOR_NOT_SELECTED_REASON");
    assert_eq!(TASK_DIGEST_ENV, "VELNOR_NOOP_TASK_DIGEST");
    assert_eq!(EXIT_CODE_ENV, "VELNOR_EXIT_CODE");
    assert_eq!(TASK_ID_ENV, "VELNOR_TASK_ID");
}

#[test]
fn noop_rejects_malformed_run_key_before_path_join() {
    let (plan, task_digest) = fixture_plan();
    let temp = staged_run(&plan);
    let request = NoOpRequest {
        reason: NotSelectedReason::Unsupported,
        task_digest,
    };
    // Symmetry with the exec path: the run key is validated before
    // `load_plan` joins it into a path, so traversal keys never reach
    // the filesystem (unreachable via `resolve_run_key` today, which
    // validates or re-derives digits-only keys).
    for bad in [
        "",
        "LOCAL",
        "r1-a",
        "../evil",
        "local/../../evil",
        "r1-a1/x",
    ] {
        let err = write_noop_report_to(bad, CLIPPY, 0, &request, temp.path()).expect_err("run key");
        assert!(
            err.to_string().contains("malformed_run_key"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn noop_digest_key_is_disjoint_from_exec_digest_key() {
    assert_ne!(
        TASK_DIGEST_ENV, OBLIGATION_TASK_DIGEST_ENV,
        "exec steps bake the obligation digest into every obligation env; aliasing makes the report op fail noop_half_present on every executed task"
    );
}
