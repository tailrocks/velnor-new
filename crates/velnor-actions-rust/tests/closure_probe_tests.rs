//! Input consumers cannot be inferred from config filename or task kind.
use super::{probe_cargo_config, probe_declared, probe_nextest_config};
use velnor_actions_contract::Provenance;

fn fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("velnor-rust-probes-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("create probe fixture");
    root
}

#[test]
fn cargo_config_external_input_is_unknown() {
    let root = fixture().join("cargo");
    std::fs::create_dir_all(root.join("crates/a/.cargo")).expect("config dir");
    std::fs::write(
        root.join("crates/a/.cargo/config.toml"),
        "[build]\nrustflags = [\"-C\", \"link-arg=-Tignored.ld\"]\n",
    )
    .expect("config");
    assert!(matches!(probe_cargo_config(&root, "crates/a/Cargo.toml"),
        Provenance::Unknown { ref reason } if reason.starts_with("cargo_config_consumed_inputs_unproven")));
}

#[test]
fn nextest_simple_profiles_known_but_scripts_unknown() {
    let root = fixture().join("nextest");
    std::fs::create_dir_all(root.join(".config")).expect("config dir");
    let path = root.join(".config/nextest.toml");
    std::fs::write(
        &path,
        "[profile.ci]\nretries = 0\ntest-threads = \"num-cpus\"\n",
    )
    .expect("profile");
    assert!(matches!(
        probe_nextest_config(&root, None),
        Provenance::Known { .. }
    ));
    std::fs::write(
        &path,
        "[scripts.setup.native]\ncommand = \"ignored-setup.sh\"\n",
    )
    .expect("script");
    assert!(matches!(
        probe_nextest_config(&root, None),
        Provenance::Unknown { .. }
    ));
}

#[cfg(unix)]
#[test]
fn declared_symlink_is_unknown_and_executable_mode_is_bound() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture().join("declared");
    std::fs::create_dir_all(&root).expect("fixture dir");
    let path = root.join("fixture.sh");
    std::fs::write(&path, "exit 0\n").expect("fixture");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let before = probe_declared(&root, "fixture.sh");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("mode");
    assert_ne!(before, probe_declared(&root, "fixture.sh"));
    let link = root.join("fixture-link.sh");
    if link.symlink_metadata().is_ok() {
        std::fs::remove_file(&link).expect("old link");
    }
    std::os::unix::fs::symlink(&path, &link).expect("symlink");
    assert!(matches!(
        probe_declared(&root, "fixture-link.sh"),
        Provenance::Unknown { .. }
    ));
}
