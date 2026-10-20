use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_13_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.15");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "e3b186d4c70627f27037be615becaf4acfb30e41fb6a1819486717dc94e4e2a9"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.15");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "ea45cc6e787d412deb8e069eec57875c71863e33a36acf69dd59791c27c9c073"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
