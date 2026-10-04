//! Native consumers restore public source bytes and never export project writes.
use super::*;
use crate::workloads::cache_eligibility::NativeNpmSource;
use velnor_actions_contract::StepKind;
use velnor_actions_mise::ToolCatalog;

fn sources() -> Vec<NativeNpmSource> {
    vec![NativeNpmSource { name: "public-package".to_owned(), version: "1.2.3".to_owned(),
        resolved: "https://registry.npmjs.org/public-package/-/public-package-1.2.3.tgz".to_owned(),
        integrity: "sha512-LCpQRaCgZUZ2pSPVoon4GB+7kQmwPoFYikmLkF0FXqdAJVV3bjCZahRWiL5jPlsIm5qrqI88s4Zgjwaiu/sTlQ==".to_owned() }]
}

#[test]
fn consumer_restores_exact_source_cohort_without_export_or_task_outputs() {
    let key = npm_source_job::source_key(&sources(), &ToolCatalog::pinned(), "ubuntu-26.04")
        .expect("source key");
    let step = npm_source_job::restore_step(&key).expect("restore");
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("restore action");
    };
    assert_eq!(with["path"], npm::payload_paths().join("\n"));
    assert!(with["key"].starts_with("velnor-native-v3-npm-downloads-"));
    assert!(!with["key"].contains("${{"));
    assert!(with["restore-keys"].ends_with("-snapshot-"));
    assert!(!with["path"].contains("node_modules"));
    assert!(!with["path"].contains("index-v5"));
}

#[test]
fn source_compatibility_binds_exact_tuples_native_owner_and_platform() {
    let catalog = ToolCatalog::pinned();
    let sources = sources();
    let original = npm_source_job::source_key(&sources, &catalog, "ubuntu-26.04").expect("key");
    assert_ne!(
        original,
        npm_source_job::source_key(&sources, &catalog, "macos-26").expect("platform")
    );
    let mut changed = sources;
    changed[0].version = "1.2.4".to_owned();
    assert_ne!(
        original,
        npm_source_job::source_key(&changed, &catalog, "ubuntu-26.04").expect("tuple")
    );
}

#[test]
fn native_execution_homes_are_independent_of_optional_restore_admission() {
    for kind in ["bun_ci", "gradle_check", "node_ci"] {
        assert!(!task_env(&format!("stack/workload/app/install/{kind}")).is_empty());
    }
    for kind in [
        "docker_build",
        "ruby_syntax",
        "shellcheck",
        "reuse",
        "swift_test",
    ] {
        assert!(task_env(&format!("stack/workload/app/install/{kind}")).is_empty());
    }
    assert!(task_env("stack/rust/crate/install/node_ci").is_empty());
}

#[test]
fn source_producer_uses_registered_fixed_helper_and_argv_descriptors() {
    let step = npm_proof::source_step(
        &sources(),
        &ToolCatalog::pinned(),
        velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
        "/pinned/node",
        "literal-source-key",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("source helper");
    let StepKind::SourceBoundHelper { invocation, env } = step.kind else {
        panic!("qualified helper");
    };
    assert_eq!(invocation.args()[0], "/pinned/node");
    assert_eq!(
        serde_json::from_str::<NativeNpmSource>(&invocation.args()[4]).expect("immutable tuple"),
        sources()[0]
    );
    assert!(!env.contains_key("VELNOR_NPM_SOURCE_CANDIDATES"));
    assert_eq!(env["PATH"], "/usr/bin:/bin");
    assert!(!env.contains_key("PYTHONPATH"));
}
