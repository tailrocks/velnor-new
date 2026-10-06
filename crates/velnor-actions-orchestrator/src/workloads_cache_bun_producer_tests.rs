use super::{NATIVE_PRODUCER, PUBLIC_PROOF, compiled_helper, executable, producer_body};
use crate::workloads::cache_eligibility::NativeNpmSource;

fn source() -> NativeNpmSource {
    NativeNpmSource {
        name: "a".to_owned(),
        version: "1.2.3".to_owned(),
        resolved: "https://registry.npmjs.org/a/-/a-1.2.3.tgz".to_owned(),
        integrity: format!("sha512-{}==", "A".repeat(86)),
    }
}

#[test]
fn source_factory_rejects_empty_private_or_rebound_descriptors() {
    assert!(compiled_helper(&[], "0.1.0").is_err());
    let mut descriptor = source();
    descriptor.resolved = "https://token@registry.npmjs.org/a/-/a-1.2.3.tgz".to_owned();
    assert!(compiled_helper(&[descriptor], "0.1.0").is_err());
    let helper = compiled_helper(&[source()], "0.1.0").expect("compiled helper");
    assert_eq!(helper.invocation().args()[0], executable());
    assert_eq!(
        helper.invocation().args()[1],
        velnor_actions_mise::catalog::BUN_VERSION
    );
    assert!(helper.invocation().installed_selectors().is_empty());
    assert!(helper.source().contains("/usr/bin/python3"));
    assert!(helper.source().contains("--frozen-lockfile"));
    assert!(helper.source().contains("--ignore-scripts"));
}

#[test]
fn changed_source_is_a_distinct_exact_helper_invocation() {
    let old = compiled_helper(&[source()], "0.1.0").expect("old source");
    let mut changed = source();
    changed.version = "2.0.0".to_owned();
    changed.resolved = "https://registry.npmjs.org/a/-/a-2.0.0.tgz".to_owned();
    let changed = compiled_helper(&[changed], "0.1.0").expect("changed source");
    assert_ne!(old.invocation(), changed.invocation());
    assert_eq!(old.source(), changed.source());
}

#[test]
fn complete_source_authority_and_native_failure_disable_optional_publication() {
    let temp = tempfile::tempdir().expect("temp");
    let public = temp.path().join("public.py");
    let native = temp.path().join("native.py");
    std::fs::write(&public, PUBLIC_PROOF).expect("public source");
    std::fs::write(&native, NATIVE_PRODUCER).expect("native source");
    let status = std::process::Command::new("/usr/bin/python3")
        .args([
            "-I",
            "-c",
            include_str!("workloads_cache_bun_producer_fixture.py"),
        ])
        .arg(public)
        .arg(native)
        .arg(velnor_actions_mise::catalog::BUN_VERSION)
        .status()
        .expect("fixture runner");
    assert!(status.success());
}

#[test]
fn generated_optional_wrapper_does_not_authorize_failed_producer() {
    let temp = tempfile::tempdir().expect("temp");
    let output = temp.path().join("output");
    let status = std::process::Command::new("/bin/sh")
        .args(["-e", "-c", &producer_body().expect("body")])
        .env("RUNNER_TEMP", temp.path())
        .env("GITHUB_OUTPUT", &output)
        .status()
        .expect("producer wrapper");
    assert!(!status.success());
    let output = std::fs::read_to_string(output).expect("safe result");
    assert_eq!(
        output,
        "verified=false\nerror=SOURCE_VERIFICATION_FAILED\npublic-packages=0\nsource-error=source_descriptor_invalid\n"
    );
}
