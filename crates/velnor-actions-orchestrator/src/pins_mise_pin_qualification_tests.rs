use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_13_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.17");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "fe6dd17e68e09682b31d2e7bb7bb755d22636ac1af5d94621e300f2ca9e9fe8a"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.17");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "dac5d6db5e315d8e4f096c6ead9279d342e09fdcb5c373d0df5c7dcce8195746"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
