use velnor_runner_core::{
    AcquireIntentId, Capacity, CleanupProof, Epoch, OwnedIds, WorkerId, WorkerState,
};

use crate::reconcile::release_permitted;
use crate::{HostError, ReleaseFact};

fn apply_release(
    capacity: &mut Capacity,
    worker: WorkerId,
    fact: ReleaseFact,
    proof: &CleanupProof,
) -> Result<(), HostError> {
    if release_permitted(fact) {
        capacity
            .release(worker, proof)
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

#[test]
fn release_permitted_gates_capacity_release() -> Result<(), HostError> {
    let worker = WorkerId::new(1).map_err(|_| HostError::Journal)?;
    let intent_id = AcquireIntentId::new(1).map_err(|_| HostError::Journal)?;
    let mut capacity = Capacity::new(1);
    capacity
        .reserve(worker, intent_id, Epoch::new(1), 1, 1)
        .map_err(|_| HostError::Journal)?;
    let owned = OwnedIds {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    };
    capacity
        .store(
            worker,
            WorkerState::Cleaning {
                epoch: Epoch::new(1),
                owned: owned.clone(),
            },
        )
        .map_err(|_| HostError::Journal)?;
    let proof = CleanupProof {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    };
    assert!(!release_permitted(ReleaseFact::Pending));
    assert!(!release_permitted(ReleaseFact::Uncertain));
    assert!(!release_permitted(ReleaseFact::DefiniteFailure));
    assert!(release_permitted(ReleaseFact::ProvenCleanup));
    apply_release(&mut capacity, worker, ReleaseFact::Pending, &proof)?;
    apply_release(&mut capacity, worker, ReleaseFact::Uncertain, &proof)?;
    apply_release(&mut capacity, worker, ReleaseFact::DefiniteFailure, &proof)?;
    assert_eq!(capacity.occupancy(), 1);
    apply_release(&mut capacity, worker, ReleaseFact::ProvenCleanup, &proof)?;
    assert_eq!(capacity.occupancy(), 0);
    Ok(())
}
