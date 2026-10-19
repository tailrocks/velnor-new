use crate::journal::{HostHelperRole, ProbeWork, ResourceProbePurpose};

use super::{ActiveRecoveryTarget, active_recovery_target};

#[test]
fn active_recovery_routes_each_typed_work_item() {
    let capacity = ProbeWork::ResourceProbe {
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: "a".repeat(64),
        request_context: None,
    };
    assert_eq!(
        active_recovery_target(HostHelperRole::ResourceProbe, &capacity),
        Ok(ActiveRecoveryTarget::ResourceProbe)
    );
    assert_eq!(
        active_recovery_target(
            HostHelperRole::ExecutionPublication,
            &ProbeWork::ExecutionPublication,
        ),
        Ok(ActiveRecoveryTarget::ExecutionPublication)
    );
    assert!(
        active_recovery_target(
            HostHelperRole::ResourceProbe,
            &ProbeWork::ExecutionPublication
        )
        .is_err()
    );
    assert!(active_recovery_target(HostHelperRole::ExecutionPublication, &capacity,).is_err());
}
