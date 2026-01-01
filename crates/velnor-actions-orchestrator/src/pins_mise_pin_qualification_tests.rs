use super::tests::config_with;
use super::*;
use std::collections::BTreeMap;

#[test]
fn qualification_resolves_2026_10_5_pins_by_native_runner_target() {
    let pins = resolve_mise_pin_qualification(&config_with(BTreeMap::new()))
        .expect("candidate pins resolve");
    assert_eq!(pins.linux_x86_64_setup.version, "2026.10.5");
    assert_eq!(
        pins.linux_x86_64_setup.sha256,
        "8a223b5f8ca71100220a3e5bef259614c348e7b1d80e6b15c2a9c9aa3affe5e4"
    );
    assert_eq!(pins.macos_x86_64_setup.version, "2026.10.5");
    assert_eq!(
        pins.macos_x86_64_setup.sha256,
        "204c7d64e8b0b62c0a95847ab6442bf23bf88247d52967e99c5cad53f56eaa0c"
    );
    assert!(pins.linux_x86_64_setup.uses.ends_with(MISE_ACTION_SHA));
    assert_eq!(pins.linux_x86_64_setup.uses, pins.macos_x86_64_setup.uses);
}
