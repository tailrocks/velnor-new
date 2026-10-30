use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_19_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.19");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.19");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
