use super::resolve;
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_generator::GeneratorReleasePins;
use velnor_actions_workflow_steps::setup::MiseSetup;

fn generator_pins() -> GeneratorReleasePins {
    GeneratorReleasePins {
        linux_x86_64_setup: setup('a', "2026.9.18", 'b'),
        macos_arm64_setup: setup('c', "2026.9.19", 'd'),
        macos_x86_64_setup: setup('e', "2026.9.20", 'f'),
        install_gate_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_gh_argv: vec!["mise".to_owned(), "install".to_owned()],
        build_argv: vec!["mise".to_owned(), "exec".to_owned(), "--".to_owned()],
        macos_x86_64_cross_build_argv: vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "--target".to_owned(),
            ReleaseTarget::MacosX86_64.triple().to_owned(),
        ],
        install_macos_x86_64_target_argv: vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "rustup".to_owned(),
            "target".to_owned(),
            "add".to_owned(),
            ReleaseTarget::MacosX86_64.triple().to_owned(),
        ],
        actionlint_argv: vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "actionlint".to_owned(),
        ],
        zizmor_argv: vec!["mise".to_owned(), "exec".to_owned(), "zizmor".to_owned()],
        gh_argv: vec!["mise".to_owned(), "exec".to_owned(), "gh".to_owned()],
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
fn product_commands_are_catalog_pinned_and_legacy_inputs_are_preserved() {
    let generator = generator_pins();
    let pins = resolve(&generator).expect("valid catalog inputs");
    let catalog = ToolCatalog::pinned();

    assert_eq!(
        pins.setup_for(ReleaseTarget::LinuxX86_64),
        &generator.linux_x86_64_setup
    );
    assert_eq!(
        pins.setup_for(ReleaseTarget::MacosArm64),
        &generator.macos_arm64_setup
    );
    assert_eq!(
        pins.setup_for(ReleaseTarget::MacosX86_64),
        &generator.macos_x86_64_setup
    );
    assert_eq!(
        pins.install_gate_tools_argv,
        generator.install_gate_tools_argv
    );
    assert_eq!(
        pins.install_build_tools_argv,
        generator.install_build_tools_argv
    );
    assert_eq!(pins.install_gh_argv, generator.install_gh_argv);
    assert_eq!(pins.build_argv, generator.build_argv);
    assert_eq!(
        pins.intel_build_argv,
        generator.macos_x86_64_cross_build_argv
    );
    assert_eq!(
        pins.install_intel_target_argv,
        generator.install_macos_x86_64_target_argv
    );
    assert_eq!(pins.actionlint_argv, generator.actionlint_argv);
    assert_eq!(pins.gh_argv, generator.gh_argv);
    assert_eq!(pins.rust_version, generator.rust_version);
    assert_eq!(pins.mr_boxington_version, generator.mr_boxington_version);

    assert!(pins.install_qualify_tools_argv.starts_with(&[
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
    ]));
    assert!(pins.install_qualify_tools_argv.contains(&format!(
        "{}@{}",
        PinnedTool::Zizmor.tool_name(),
        catalog.version(PinnedTool::Zizmor)
    )));
    assert!(pins.install_runner_build_tools_argv.contains(&format!(
        "{}@{}",
        PinnedTool::Rust.tool_name(),
        catalog.version(PinnedTool::Rust)
    )));
    assert_eq!(
        pins.runner_build_argv.first().map(String::as_str),
        Some("mise")
    );
    assert!(pins.runner_build_argv.contains(&"cargo".to_owned()));
    assert!(
        pins.runner_build_argv
            .contains(&"crates/tools/velnor-runner-cli/Cargo.toml".to_owned())
    );
    assert_eq!(pins.zizmor_argv.first().map(String::as_str), Some("env"));
    assert!(
        pins.zizmor_argv
            .windows(2)
            .any(|pair| pair[0] == "-u" && pair[1] == "GH_TOKEN")
    );
}

#[test]
fn runner_build_targets_existing_cli_manifest_and_binary() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../");
    let manifest = workspace.join("crates/tools/velnor-runner-cli/Cargo.toml");
    let source = std::fs::read_to_string(&manifest).expect("runner CLI manifest exists");
    assert!(
        source.contains("name = \"velnor-runner-cli\""),
        "{}",
        manifest.display()
    );
    assert!(
        source.contains("name = \"velnor-host\""),
        "{}",
        manifest.display()
    );
}
