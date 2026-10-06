use super::*;

/// Conclusions over one unsorted inventory.
fn conclusions(inventory: &[&str]) -> NeedsConclusions {
    NeedsConclusions {
        required_job: "required".to_owned(),
        inventory: inventory.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn expected_env_emits_sorted_inventory_json() {
    let (key, value) = conclusions(&["zizmor", "plan", "alint"]).expected_env();
    assert_eq!(key, NEEDS_EXPECTED_ENV);
    assert_eq!(value, "[\"alint\",\"plan\",\"zizmor\"]");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&value).expect("valid json"),
        ["alint", "plan", "zizmor"],
    );
}

/// Minimal job with the given `needs` for inventory tests.
fn job_with_needs(needs: &[&str]) -> Job {
    Job {
        display_name: "Test".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: crate::workflow::timeout::JobTimeout::VALIDATOR,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    }
}

#[test]
fn inventory_comes_from_gate_needs_not_job_set() {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), job_with_needs(&[]));
    jobs.insert("required".to_owned(), job_with_needs(&["plan"]));
    jobs.insert("publish-baseline".to_owned(), job_with_needs(&["required"]));
    jobs.insert("unrelated".to_owned(), job_with_needs(&[]));
    let conclusions =
        NeedsConclusions::from_finalized_jobs("required", &jobs).expect("gate with needs derives");
    assert_eq!(conclusions.inventory, vec!["plan".to_owned()]);
    assert!(conclusions.gate_matches(&jobs));
    let (_, value) = conclusions.expected_env();
    assert_eq!(value, "[\"plan\"]");
}

#[test]
fn inventory_rejects_missing_gate_and_empty_needs() {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), job_with_needs(&[]));
    let Err(err) = NeedsConclusions::from_finalized_jobs("required", &jobs) else {
        panic!("missing gate fails");
    };
    assert!(err.to_string().contains("missing_gate"), "{err}");
    jobs.insert("required".to_owned(), job_with_needs(&[]));
    let Err(err) = NeedsConclusions::from_finalized_jobs("required", &jobs) else {
        panic!("empty needs fail");
    };
    assert!(err.to_string().contains("empty_inventory"), "{err}");
}

#[test]
fn expected_env_handles_edge_inventories() {
    let (key, value) = conclusions(&["plan"]).expected_env();
    assert_eq!(
        (key.as_str(), value.as_str()),
        (NEEDS_EXPECTED_ENV, "[\"plan\"]")
    );
    let (_, value) = conclusions(&[]).expected_env();
    assert_eq!(value, "[]", "empty inventory stays explicit");
}
