// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{
    RETIRED, TestResult, fairness::prepare_initializer, restarted_with_removed_pairs, seed,
};
use crate::host_controller::loop_driver::polling::{PositivePollContext, pre_jit};
use crate::launch::bootstrap::unbound_pair_fixtures::seed::Pair;

const SKIPPED: Pair = Pair {
    reservation: 6,
    bound: false,
};
const STALLED: Pair = Pair {
    reservation: 4,
    bound: false,
};
const HEALTHY: Pair = Pair {
    reservation: 2,
    bound: false,
};
const GUARD: Duration = Duration::from_secs(15);
const ABORT: &str = "SELECT abort_state || '/' || abort_claim_generation || '/' || abort_lease_until FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";
const LIVE_ABORT: &str = "SELECT abort_state || '/' || (abort_lease_until > unixepoch()) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1";
const CHARGE: &str = "SELECT state || '/' || cpu_millicores || '/' || memory_bytes FROM capacity_reservations WHERE id = ?1";
const CHARGED: &str = "uncertain/3000/6442450944";

#[tokio::test(flavor = "current_thread")]
async fn exhausted_initializer_time_skips_later_claims_but_still_retires_an_assigned_pair()
-> TestResult<()> {
    let (restarted, controller) =
        restarted_with_removed_pairs(&[HEALTHY, STALLED, SKIPPED]).await?;
    for pair in [SKIPPED, STALLED] {
        prepare_initializer(&restarted, &controller, pair).await?;
    }
    seed::assigned_pair(&restarted.journal, &controller.prepared.scope, HEALTHY).await?;
    let skipped_abort = seed::text(&restarted.journal, ABORT, SKIPPED.reservation).await?;
    assert_eq!(skipped_abort, "not_requested/0/0");
    let skipped_objects = restarted.fixture.objects(SKIPPED);
    let stalled_objects = restarted.fixture.objects(STALLED);
    restarted
        .fixture
        .gate_container_inspection(&STALLED.helper_name());
    let context = PositivePollContext {
        config: &controller.config,
        journal: &restarted.journal,
        docker: &restarted.docker,
        selected_engine: &restarted.engine,
        worker_images: &controller.images,
        state: controller.state.path(),
        image_authority: &controller.image_authority,
        admission_ready: false,
        retirement_budget: Duration::from_secs(60),
    };
    let recovery = pre_jit::reconcile(&controller.prepared.scope, 1, &context);
    tokio::pin!(recovery);
    tokio::time::timeout(GUARD, async {
        tokio::select! {
            result = &mut recovery => {
                result?;
                Err::<(), Box<dyn std::error::Error>>("recovery returned before its initializer claimed and stalled".into())
            }
            () = restarted.fixture.hung() => Ok(()),
        }
    })
    .await??;
    assert_eq!(restarted.fixture.inspection_gate_hits(), 1);
    assert_eq!(
        seed::text(&restarted.journal, LIVE_ABORT, STALLED.reservation).await?,
        "requested/1"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(30)).await;
    tokio::time::resume();
    tokio::time::timeout(GUARD, &mut recovery).await??;
    assert_eq!(
        seed::text(&restarted.journal, ABORT, STALLED.reservation).await?,
        "uncertain/1/0"
    );
    assert_eq!(restarted.fixture.objects(STALLED), stalled_objects);
    assert_eq!(
        seed::text(&restarted.journal, CHARGE, STALLED.reservation).await?,
        CHARGED
    );
    assert_eq!(
        seed::text(&restarted.journal, seed::RESERVATION, HEALTHY.reservation).await?,
        RETIRED
    );
    assert_eq!(restarted.fixture.objects(HEALTHY), Vec::<String>::new());
    assert_eq!(
        seed::text(&restarted.journal, ABORT, SKIPPED.reservation).await?,
        skipped_abort
    );
    assert_eq!(
        seed::text(&restarted.journal, CHARGE, SKIPPED.reservation).await?,
        CHARGED
    );
    assert_eq!(restarted.fixture.objects(SKIPPED), skipped_objects);
    Ok(())
}
