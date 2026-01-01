//! Required-job conclusion parsing and serialization cases.

use velnor_actions_contract_workflow::{JobConclusion, RequiredJobResult};

#[test]
fn job_conclusions_parse_and_serialize_closed() {
    for (word, conclusion) in [
        ("success", JobConclusion::Success),
        ("failure", JobConclusion::Failure),
        ("cancelled", JobConclusion::Cancelled),
        ("skipped", JobConclusion::Skipped),
        ("neutral", JobConclusion::Neutral),
        ("missing", JobConclusion::Missing),
    ] {
        assert_eq!(JobConclusion::parse(word), Ok(conclusion));
        assert_eq!(conclusion.as_str(), word);
        let job = RequiredJobResult {
            job_id: "plan".to_owned(),
            conclusion,
        };
        let json = serde_json::to_value(&job).expect("serialize");
        assert_eq!(json["conclusion"], word);
        let back: RequiredJobResult = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.conclusion, conclusion);
    }
    for bad in ["", "SUCCESS", "timed_out", "stale", "action_required"] {
        let err = JobConclusion::parse(bad).expect_err("unknown");
        assert!(err.to_string().contains("unknown_conclusion"), "{err}");
    }
}
