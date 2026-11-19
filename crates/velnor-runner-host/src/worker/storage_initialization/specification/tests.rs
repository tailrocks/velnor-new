// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{StorageInitializationImage, StorageInitializationLimits, specification};
use crate::config::RunnerPlatform;
use crate::worker::ResourceBudgetConfig;

#[test]
fn the_initializer_is_created_for_the_images_platform() -> Result<(), Box<dyn std::error::Error>> {
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()?;
    let limits = StorageInitializationLimits::from_reserved_budget(budget, 500, 64 << 20, 64)?;
    for platform in [RunnerPlatform::LinuxAmd64, RunnerPlatform::LinuxArm64] {
        let image = StorageInitializationImage {
            runtime_id: format!("sha256:{}", "d".repeat(64)),
            source_sha: "e".repeat(40),
            platform,
        };
        let create = specification("op-1", "home", "work", "meta", &image, limits)?;
        assert_eq!(create.options.platform, platform.as_str());
    }
    Ok(())
}
