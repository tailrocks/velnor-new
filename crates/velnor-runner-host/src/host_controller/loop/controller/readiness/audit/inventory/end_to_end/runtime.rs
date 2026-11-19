// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use tokio::time::Instant;

use super::{RefreshDependencies, SettlingAges, TestResult, audit_inventory_with_remote, engine, world};

const STALL: Duration = Duration::from_millis(1500);

#[tokio::test(flavor = "current_thread")]
async fn a_stalled_remote_audit_does_not_block_the_controller_runtime() -> TestResult<()> {
    let world = world(engine::healthy()?).await?;
    let started = Instant::now();
    let ticker = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        started.elapsed()
    });
    let stalled = |_: &_, _: Option<&_>, _: i64| {
        std::thread::sleep(STALL);
        Ok(true)
    };
    audit_inventory_with_remote(
        &world.docker,
        &world.journal,
        &world.config,
        &world.seeded.selected,
        std::slice::from_ref(&world.seeded.poll),
        &RefreshDependencies {
            remote_audit: &stalled,
            settling: &SettlingAges::default(),
        },
    )
    .await?;
    assert!(ticker.await? < STALL / 2);
    Ok(())
}
