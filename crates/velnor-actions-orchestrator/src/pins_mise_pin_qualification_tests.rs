use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_13_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.18");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.18");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "72e1ced3bac89d0e0aa28b04bd22f8941c111d3b20c74a77bfb6a9486f25f7d3"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
