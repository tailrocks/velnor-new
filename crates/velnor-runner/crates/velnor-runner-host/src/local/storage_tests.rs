// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::{StorageProvisioningRole, StorageRole};

use super::super::plan::Plan;
use super::{GIB, Sizes, charge_bytes, driver_opts, on_tmpfs, plan};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn six_gib_home() -> Sizes {
    Sizes {
        home: 6 * GIB,
        ..Sizes::default()
    }
}

fn plan_json(storage: &str) -> String {
    format!(
        "{{\"environment\":\"e\",\"authority\":\"http://127.0.0.1:53114\",\"token\":\"{TOKEN}\",\"docker\":\"unix:///var/run/docker.sock\",\"platform\":\"linux/arm64\",\"runner_image\":\"a\",\"dind_image\":\"b\",\"limits\":{{\"runner_cpu_millicores\":1500,\"runner_memory_bytes\":1073741824,\"dind_cpu_millicores\":500,\"dind_memory_bytes\":1073741824}}{storage},\"sides\":[{{\"side\":\"trusted\",\"network\":\"velnor-local-x\",\"pairs\":1}}]}}"
    )
}

#[test]
fn plan_charges_the_sizes_it_is_given() {
    let charged = |sizes: &Sizes, role| {
        plan(1, sizes)
            .roles()
            .iter()
            .find(|charge| charge.role() == role)
            .map(StorageProvisioningRole::charge_bytes)
    };
    let sizes = six_gib_home();
    assert_eq!(charged(&sizes, StorageRole::RunnerHome), Some(6 * GIB));
    assert_eq!(charged(&sizes, StorageRole::RunnerWork), Some(16 * GIB));
    assert_eq!(charged(&sizes, StorageRole::RunnerTmp), Some(2 * GIB));
    assert_eq!(charged(&sizes, StorageRole::DindTmp), Some(2 * GIB));
    assert_eq!(charged(&sizes, StorageRole::DindData), Some(16 * GIB));
    assert_eq!(
        charge_bytes(StorageRole::RunnerHome, &Sizes::default()),
        2 * GIB
    );
}

#[test]
fn home_volume_options_carry_the_size() {
    let options = driver_opts(StorageRole::RunnerHome, &six_gib_home())
        .expect("options")
        .expect("home is a tmpfs");
    let mount = options.get("o").expect("mount options");
    assert!(mount.contains("size=6442450944"), "{mount}");
    assert!(mount.contains("nr_inodes=393216"), "{mount}");
    assert!(mount.contains("uid=1000"), "{mount}");
    let work = driver_opts(StorageRole::RunnerWork, &six_gib_home())
        .expect("options")
        .expect("work is a tmpfs");
    assert!(work["o"].contains("size=17179869184"), "{}", work["o"]);
    assert_eq!(
        driver_opts(StorageRole::RunnerRoot, &six_gib_home()),
        Ok(None)
    );
}

#[test]
fn plan_storage_is_optional_and_bounded() {
    let absent = Plan::parse(plan_json("").as_bytes()).expect("absent storage");
    assert_eq!(absent.storage, Sizes::default());
    let storage = |home: u64| {
        format!(
            ",\"storage\":{{\"home_bytes\":{home},\"work_bytes\":17179869184,\"tmp_bytes\":2147483648}}"
        )
    };
    let given = Plan::parse(plan_json(&storage(6 * GIB)).as_bytes()).expect("6 GiB home");
    assert_eq!(given.storage, six_gib_home());
    for home in [512 << 20, 65 * GIB, 0] {
        let refused = Plan::parse(plan_json(&storage(home)).as_bytes());
        assert!(matches!(refused, Err(HostError::Config)), "{home}");
    }
}

#[test]
fn the_metadata_volume_is_not_a_size_limited_tmpfs() {
    assert!(!on_tmpfs(StorageRole::EngineMetadata));
}
