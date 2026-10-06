use super::*;

/// Parse results plus errors for one channel value.
fn parsed(
    needs: Option<&str>,
    expected: Option<&str>,
) -> (Vec<String>, Vec<serde_json::Value>, Vec<String>) {
    let mut errors = Vec::new();
    let (inventory, results) = parse_needs(needs, expected, &mut errors);
    (inventory, results, errors)
}

#[test]
fn direct_and_to_json_shapes_parse_and_sort() {
    let (inventory, results, errors) = parsed(
        Some(r#"{"zeta":"success","plan":{"result":"failure"},"alpha":{"result":"bogus"}}"#),
        Some(r#"["plan","zeta"]"#),
    );
    assert_eq!(inventory, ["plan", "zeta"]);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["job_id"], "plan");
    assert_eq!(results[0]["conclusion"], "failure");
    assert_eq!(
        errors,
        ["bad_needs_result:alpha"],
        "unknown conclusions corrupt the channel"
    );
    let (inventory, _, errors) = parsed(
        Some(r#"{"a":"cancelled","b":"skipped"}"#),
        Some(r#"["a","b"]"#),
    );
    assert_eq!(inventory, ["a", "b"]);
    assert!(errors.is_empty());
}

#[test]
fn matrix_driver_job_is_excluded() {
    let channel = format!(r#"{{"plan":"success","{TASK_JOB_ID}":"success"}}"#);
    let (inventory, results, errors) = parsed(Some(&channel), Some(r#"["plan"]"#));
    assert_eq!(inventory, ["plan"]);
    assert_eq!(results.len(), 1);
    assert!(errors.is_empty());
}

#[test]
fn garbage_empty_and_bad_job_inputs_fail_closed() {
    for needs in [None, Some(""), Some("   ")] {
        let (inventory, results, errors) = parsed(needs, None);
        assert!(inventory.is_empty());
        assert!(results.is_empty());
        assert_eq!(errors, ["missing_needs_channel"]);
    }
    for needs in ["not json", "[1,2]", "42", r#""str""#] {
        let (inventory, results, errors) = parsed(Some(needs), None);
        assert!(inventory.is_empty());
        assert!(results.is_empty());
        assert_eq!(errors, ["unparsable_needs"], "{needs}");
    }
    let (inventory, results, errors) = parsed(Some("{}"), None);
    assert!(inventory.is_empty());
    assert!(results.is_empty());
    assert_eq!(errors, ["empty_needs"]);
    let (inventory, _, errors) =
        parsed(Some(r#"{"":"success","ok":"success"}"#), Some(r#"["ok"]"#));
    assert_eq!(inventory, ["ok"]);
    assert_eq!(errors, ["bad_needs_job"]);
    let (_, _, errors) = parsed(Some(r#"{"a":{},"b":{"result":7}}"#), Some(r#"["a","b"]"#));
    assert_eq!(
        errors,
        [
            "bad_needs_result:a",
            "bad_needs_result:b",
            "needs_inventory_mismatch"
        ]
    );
}
