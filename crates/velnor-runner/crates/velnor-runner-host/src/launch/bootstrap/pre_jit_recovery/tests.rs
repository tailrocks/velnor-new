use super::PreJitCleanupPhase;

#[path = "tests/capacity_pipeline.rs"]
mod capacity_pipeline;

#[test]
fn cleanup_phases_have_one_ordered_successor() {
    assert_eq!(
        PreJitCleanupPhase::Claimed.successor(),
        Some(PreJitCleanupPhase::BootstrapAbsent)
    );
    assert_eq!(
        PreJitCleanupPhase::BootstrapAbsent.successor(),
        Some(PreJitCleanupPhase::InitializerAbsent)
    );
    assert_eq!(
        PreJitCleanupPhase::InitializerAbsent.successor(),
        Some(PreJitCleanupPhase::StorageAbsent)
    );
    assert_eq!(
        PreJitCleanupPhase::StorageAbsent.successor(),
        Some(PreJitCleanupPhase::NetworkAbsent)
    );
    assert_eq!(PreJitCleanupPhase::NetworkAbsent.successor(), None);
    assert_eq!(PreJitCleanupPhase::Settled.successor(), None);
}
