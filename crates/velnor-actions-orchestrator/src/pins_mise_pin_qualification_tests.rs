use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_8_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.8");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "c72477ae6c3a8e7d817a322f43da70e4643e75ee5c21265f1c19c894273b7428"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.8");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "a122fd04db5f2da07d24528ed61109bc82142ca44e821d4faafa05650f6ba627"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
