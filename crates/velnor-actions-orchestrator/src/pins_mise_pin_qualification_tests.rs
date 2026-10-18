use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_13_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.13");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "94c45523c7b6760b7da704f16051171ca32e8447a849a02266a439a4455e9a12"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.13");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "4e4a996f4747eb59d55497fdd4b0d60d3add9a06c0dc5d65a70402015ee445d0"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
