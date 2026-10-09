use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_6_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.6");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "3f44343eebc7e0d6623bcea46e304864f02dff648edd75c82871b53cc697b366"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.6");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "70e1407e2fdc7a19f94db35745a8e5885b0e4bbdbfb34bfb7e3619d6230a8f70"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
