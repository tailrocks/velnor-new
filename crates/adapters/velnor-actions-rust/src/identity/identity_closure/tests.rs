use super::*;
use crate::task_identity::DigestSlot;

/// Extension inputs with `lock`, `nextest`, and `rerun` supplied.
fn inputs(
    lock: crate::task_identity::DigestSlot,
    nextest: crate::task_identity::DigestSlot,
    rerun: Option<&[String]>,
    build_script: bool,
) -> GroupExtensionInputs<'_> {
    GroupExtensionInputs {
        package_id: "demo",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: "graph",
        targets: &[],
        config_digest: "config",
        lock_digest: lock,
        nextest_digest: nextest,
        archive_source: None,
        rerun_inputs: rerun,
        has_build_script: build_script,
    }
}

/// Minimal group of `kind` with the Nextest runner.
fn group(kind: crate::tasks::TaskKind) -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust/root/nextest/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: crate::profile::CompileDriver::Cargo,
        test_runner: crate::profile::TestRunner::CargoNextest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: crate::profile::NextestProfile::Default,
        run_ignored: None,
    }
}

#[test]
fn unresolved_inventory_names_unknown_inputs() {
    let ext = group(crate::tasks::TaskKind::Nextest).identity_extension(&inputs(
        DigestSlot::Unknown("unprobed".to_owned()),
        DigestSlot::Unknown("unprobed".to_owned()),
        Some(&[]),
        false,
    ));
    assert_eq!(
        unresolved_inputs(&ext),
        vec![UnresolvedInput::Lockfile, UnresolvedInput::NextestConfig]
    );
    let ext = group(crate::tasks::TaskKind::Test).identity_extension(&inputs(
        DigestSlot::Known("lock".to_owned()),
        DigestSlot::Unknown("unprobed".to_owned()),
        Some(&[]),
        false,
    ));
    assert!(unresolved_inputs(&ext).is_empty());
    let build = group(crate::tasks::TaskKind::Build).identity_extension(&inputs(
        DigestSlot::Known("lock".to_owned()),
        DigestSlot::Known("nextest".to_owned()),
        Some(&[]),
        false,
    ));
    assert_eq!(
        unresolved_inputs(&build),
        vec![UnresolvedInput::ArchiveSource]
    );
    let script = group(crate::tasks::TaskKind::Clippy).identity_extension(&inputs(
        DigestSlot::Known("lock".to_owned()),
        DigestSlot::Unknown("unprobed".to_owned()),
        None,
        true,
    ));
    assert!(unresolved_inputs(&script).contains(&UnresolvedInput::RerunInputs));
}

#[test]
fn proven_absence_binds_without_blocking() {
    let ext = group(crate::tasks::TaskKind::Nextest).identity_extension(&inputs(
        DigestSlot::AbsentProven("not_found:Cargo.lock".to_owned()),
        DigestSlot::AbsentProven("not_found:.config/nextest.toml".to_owned()),
        Some(&[]),
        false,
    ));
    assert!(unresolved_inputs(&ext).is_empty());
    assert_eq!(ext.lock_digest, None);
    assert_eq!(ext.nextest_digest, None);
    let unknown = group(crate::tasks::TaskKind::Nextest).identity_extension(&inputs(
        DigestSlot::Unknown("unprobed".to_owned()),
        DigestSlot::AbsentProven("not_found:.config/nextest.toml".to_owned()),
        Some(&[]),
        false,
    ));
    assert_eq!(unresolved_inputs(&unknown), vec![UnresolvedInput::Lockfile]);
}

#[test]
fn unresolved_ignores_composite_spellings() {
    let mut cargo = group(crate::tasks::TaskKind::Build);
    cargo.test_runner = crate::profile::TestRunner::CargoTest;
    let mut spoofed = cargo.identity_extension(&inputs(
        DigestSlot::Known("lock".to_owned()),
        DigestSlot::Known("nextest".to_owned()),
        Some(&[]),
        false,
    ));
    spoofed.driver = "cargo+nextest-spoof".to_owned();
    spoofed.kind = "build".to_owned();
    assert!(
        !unresolved_inputs(&spoofed).contains(&UnresolvedInput::ArchiveSource),
        "substring sniffing must not resurrect: {spoofed:?}"
    );
    let mut test = group(crate::tasks::TaskKind::Test);
    test.test_runner = crate::profile::TestRunner::CargoTest;
    let mut kind_spoof = test.identity_extension(&inputs(
        DigestSlot::Unknown("unprobed".to_owned()),
        DigestSlot::Unknown("unprobed".to_owned()),
        Some(&[]),
        false,
    ));
    kind_spoof.kind = "nextest".to_owned();
    assert_eq!(
        unresolved_inputs(&kind_spoof),
        vec![UnresolvedInput::Lockfile]
    );
}

#[test]
fn verified_construction_rejects_bad_paths() {
    let clippy = group(crate::tasks::TaskKind::Clippy);
    assert!(
        clippy
            .identity_extension_verified(&inputs(
                DigestSlot::Known("l".to_owned()),
                DigestSlot::Unknown("unprobed".to_owned()),
                Some(&[]),
                false
            ))
            .is_ok()
    );
    let group = TaskGroup {
        declared_inputs: vec!["../escape".to_owned()],
        ..clippy.clone()
    };
    assert!(
        group
            .identity_extension_verified(&inputs(
                DigestSlot::Known("l".to_owned()),
                DigestSlot::Unknown("unprobed".to_owned()),
                Some(&[]),
                false
            ))
            .is_err()
    );
    assert!(normalize_identity_path("Crates/Äpfel/x.proto").is_ok());
    assert!(normalize_identity_path("a\\b").is_err());
}

#[test]
fn identity_paths_preserve_case_and_reject_malformed() {
    assert_eq!(
        normalize_identity_path("Crates/Äpfel/Cargo.toml").expect("unicode"),
        "Crates/Äpfel/Cargo.toml"
    );
    for bad in ["", "/abs/path", "a/../b", "a\\b", "a\0b", "a\nb"] {
        assert!(normalize_identity_path(bad).is_err(), "{bad:?}");
    }
}
