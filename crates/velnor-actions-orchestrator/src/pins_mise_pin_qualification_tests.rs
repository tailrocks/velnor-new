use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_13_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.14");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "50be7185bcf75929f01cc5630276bcb43ca367f7c5b176e2580903d28c611dc4"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.14");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "158cd8124bc1108efa6588679b87031c970f592230b618e11c409b55c58e7b27"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
