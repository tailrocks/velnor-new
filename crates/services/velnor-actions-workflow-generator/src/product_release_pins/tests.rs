use super::ProductReleasePins;
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_workflow_steps::setup::MiseSetup;

fn pins() -> ProductReleasePins {
    ProductReleasePins {
        linux_x86_64_setup: setup('a', "2026.9.18", 'b'),
        macos_arm64_setup: setup('c', "2026.9.19", 'd'),
        macos_x86_64_setup: setup('e', "2026.9.20", 'f'),
        install_gate_tools_argv: Vec::new(),
        install_build_tools_argv: Vec::new(),
        install_qualify_tools_argv: Vec::new(),
        install_runner_build_tools_argv: Vec::new(),
        install_gh_argv: Vec::new(),
        build_argv: Vec::new(),
        intel_build_argv: Vec::new(),
        install_intel_target_argv: Vec::new(),
        runner_build_argv: Vec::new(),
        actionlint_argv: Vec::new(),
        zizmor_argv: Vec::new(),
        gh_argv: Vec::new(),
        rust_version: "1.99.0".to_owned(),
        mr_boxington_version: "1.21.1".to_owned(),
    }
}

fn setup(seed: char, version: &str, checksum: char) -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{}", seed.to_string().repeat(40)),
        version: version.to_owned(),
        sha256: checksum.to_string().repeat(64),
    }
}

#[test]
fn setup_for_returns_the_pin_for_each_supported_release_target() {
    let pins = pins();
    assert_eq!(
        pins.setup_for(ReleaseTarget::LinuxX86_64).version,
        "2026.9.18"
    );
    assert_eq!(
        pins.setup_for(ReleaseTarget::MacosArm64).version,
        "2026.9.19"
    );
    assert_eq!(
        pins.setup_for(ReleaseTarget::MacosX86_64).version,
        "2026.9.20"
    );
}
