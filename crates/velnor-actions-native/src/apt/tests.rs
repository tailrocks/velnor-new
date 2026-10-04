use super::*;

#[test]
fn complete_support_closure_is_deterministic_and_marked() {
    let bundle = support_sources("0.1.0").expect("compiled sources");
    assert_eq!(bundle, support_sources("0.1.0").expect("same sources"));
    assert_eq!(bundle.files().len(), SUPPORT_PATHS.len());
    assert_eq!(bundle.files().len(), 2);
    assert!(
        bundle
            .files()
            .windows(2)
            .all(|files| files[0].path() < files[1].path())
    );
    for file in bundle.files() {
        assert!(SUPPORT_PATHS.contains(&file.path()));
        assert!(
            file.source()
                .lines()
                .next()
                .is_some_and(velnor_actions_contract::is_generated_marker_line)
        );
        assert!(!file.source().contains("shell=True"));
        assert!(!file.source().contains("velnor-workflow"));
    }
    let entry = bundle
        .files()
        .iter()
        .find(|file| file.path() == ".github/velnor/apt_delivery.py")
        .expect("immutable entry source");
    for module in [
        "delivery_apt_core",
        "delivery_apt_verify",
        "delivery_apt_stage_feed",
        "delivery_apt_stage_publish",
        "delivery_apt_stage",
        "delivery_apt_entry",
    ] {
        assert!(entry.source().contains(module));
    }
    assert!(!entry.source().contains("importlib"));
    assert!(!entry.source().contains("__file__"));
}

#[test]
fn malformed_version_cannot_register_sources() {
    assert!(support_sources("bad\nmarker").is_err());
}

#[test]
fn native_helper_adversarial_tests() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/apt");
    let status = std::process::Command::new("python3")
        .args([
            "-B",
            "-m",
            "unittest",
            "delivery_apt_verify_test",
            "delivery_apt_stage_test",
            "delivery_apt_entry_test",
            "delivery_apt_transport_test",
            "delivery_apt_keyring_test",
        ])
        .current_dir(directory)
        .status()
        .expect("python3 native verifier tests");
    assert!(status.success(), "native verifier adversarial tests failed");
}
