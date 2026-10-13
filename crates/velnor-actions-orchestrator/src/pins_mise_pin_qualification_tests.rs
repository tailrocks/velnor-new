use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_8_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.9");
    assert_eq!(pins.linux_x86_64_setup.sha256, MISE_BINARY_SHA256_LINUX_X64);
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.9");
    assert_eq!(pins.macos_x86_64_setup.sha256, MISE_BINARY_SHA256_MACOS_X64);
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
