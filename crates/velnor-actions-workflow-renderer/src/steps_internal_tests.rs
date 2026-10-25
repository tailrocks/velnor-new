use super::split_internal_operation;
use crate::parallel_target_seed::SEED_PARALLEL_TARGETS_OPERATION;

#[test]
fn parallel_target_seed_admits_only_its_fixed_internal_operation() {
    assert_eq!(
        split_internal_operation(SEED_PARALLEL_TARGETS_OPERATION).expect("fixed operation"),
        (
            SEED_PARALLEL_TARGETS_OPERATION,
            SEED_PARALLEL_TARGETS_OPERATION
        )
    );
    assert!(split_internal_operation("seed-parallel-targets-v1:extra").is_err());
    assert!(split_internal_operation("seed-parallel-targets-v2").is_err());
}
