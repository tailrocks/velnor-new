//! Task identity path normalization and digest compatibility.
use crate::impl_contract_ids::sample_identity;
use velnor_actions_contract::{ContractError, canonical_json_bytes, digest_b3, input_digest};

#[test]
fn input_digest_hashes_normalized_paths_and_preserves_canonical_preimage()
-> Result<(), ContractError> {
    let mut canonical = sample_identity();
    canonical.project_root = "crates/demo".to_owned();
    canonical.component_id = "crates/demo".to_owned();
    canonical.working_dir = "crates/demo/src".to_owned();
    canonical.inputs[0].path = "crates/demo/src/lib.rs".to_owned();

    let prior_canonical_digest = digest_b3(&canonical_json_bytes(&canonical)?);
    assert_eq!(input_digest(&canonical)?, prior_canonical_digest);

    let mut equivalent = canonical.clone();
    equivalent.project_root = "crates\\demo".to_owned();
    equivalent.component_id = "crates\\demo".to_owned();
    equivalent.working_dir = "crates\\demo\\src".to_owned();
    equivalent.inputs[0].path = "crates\\demo\\src\\lib.rs".to_owned();
    assert_eq!(input_digest(&equivalent)?, prior_canonical_digest);

    equivalent.inputs[0].path = "crates/../outside.rs".to_owned();
    assert!(input_digest(&equivalent).is_err());
    Ok(())
}
