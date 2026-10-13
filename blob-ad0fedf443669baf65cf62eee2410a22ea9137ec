use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_7_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.7");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.7");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "c3355f0c56d1b9fe73a2ba30e034b4e483541b25b1ad812a87440abfaeec8baa"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
