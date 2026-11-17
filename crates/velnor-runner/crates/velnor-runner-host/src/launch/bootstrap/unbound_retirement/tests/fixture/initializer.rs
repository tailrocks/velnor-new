// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Pair, Value, json};
use crate::config::RunnerPlatform;
use crate::worker::ResourceBudgetConfig;
use crate::worker::storage_initialization::{
    StorageInitializationImage, StorageInitializationLimits,
};

pub(super) const PATH: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub(super) fn inspection(pair: Pair) -> Value {
    let image_id = format!("sha256:{}", "1".repeat(64));
    let image = StorageInitializationImage::for_test(
        &image_id,
        &"1".repeat(40),
        RunnerPlatform::LinuxAmd64,
    );
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: 2_000,
        runner_memory_bytes: 4 << 30,
        dind_cpu_millicores: 1_000,
        dind_memory_bytes: 2 << 30,
        operational_memory_reserve_bytes: 1 << 30,
    }
    .validate()
    .expect("fixture resource budget");
    let limits = StorageInitializationLimits::from_reserved_budget(budget, 500, 64 << 20, 16)
        .expect("fixture helper limits");
    let volumes = pair.volumes();
    let spec = crate::worker::storage_initialization::specification::specification(
        &format!("{}-1", pair.reservation - 1),
        &volumes[1].0,
        &volumes[2].0,
        &volumes[4].0,
        &image,
        limits,
    )
    .expect("fixture helper specification");
    let mut config = serde_json::to_value(spec.config).expect("fixture helper Docker config");
    config["Env"] = json!([PATH]);
    let mounts = [
        (&volumes[1].0, "/velnor/init/home"),
        (&volumes[2].0, "/velnor/init/work"),
        (&volumes[4].0, "/velnor/init/metadata"),
    ]
    .into_iter()
    .map(|(name, destination)| json!({"Name": name, "Destination": destination, "RW": true}))
    .collect::<Vec<_>>();
    json!({
        "Image": image_id,
        "HostConfig": config["HostConfig"],
        "Config": config,
        "Mounts": mounts,
        "NetworkSettings": {"SandboxID": "", "SandboxKey": "", "Ports": {}, "Networks": {}},
    })
}
