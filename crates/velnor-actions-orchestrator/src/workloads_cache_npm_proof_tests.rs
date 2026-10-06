use super::{PROOF, source_record};
use std::{fs, process::Command};

#[test]
fn public_authority_requires_anonymous_exact_metadata_and_actual_tarball() {
    let fixture = include_str!("workloads_cache_npm_proof_fixture.py");
    assert!(
        Command::new("python3")
            .args(["-I", "-c", fixture, PROOF])
            .status()
            .expect("local public proof fixture")
            .success()
    );
}

#[test]
fn critical_failure_denies_export_and_ignores_ambient_python_injection() {
    let temp = tempfile::tempdir().expect("temp");
    let physical = temp.path().canonicalize().expect("physical temp");
    let env_file = temp.path().join("github-output");
    let marker = temp.path().join("forged");
    fs::write(
        temp.path().join("sitecustomize.py"),
        format!("open({:?}, 'w').write('forged')", marker.to_string_lossy()),
    )
    .expect("malicious startup");
    fs::write(temp.path().join("python3"), "#!/bin/sh\nexit 99\n").expect("malicious executable");
    let sources = [crate::workloads::cache_eligibility::NativeNpmSource {
        name: "public-package".to_owned(), version: "1.2.3".to_owned(),
        resolved: "https://registry.npmjs.org/public-package/-/public-package-1.2.3.tgz".to_owned(),
        integrity: "sha512-LCpQRaCgZUZ2pSPVoon4GB+7kQmwPoFYikmLkF0FXqdAJVV3bjCZahRWiL5jPlsIm5qrqI88s4Zgjwaiu/sTlQ==".to_owned(),
    }];
    let record = source_record(
        &sources,
        &velnor_actions_mise::ToolCatalog::pinned(),
        velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
        "/unavailable/pinned-node",
        "literal-source-key",
        "test",
    )
    .expect("compiled source");
    let result = Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-p",
            "-c",
            record.source(),
            "source-helper",
        ])
        .args(record.invocation().args())
        .env("PATH", temp.path())
        .env("PYTHONPATH", temp.path())
        .env("PYTHONHOME", temp.path())
        .env("RUNNER_TEMP", &physical)
        .env("npm_config_cache", physical.join("velnor/native/npm"))
        .env("GITHUB_OUTPUT", &env_file)
        .status()
        .expect("isolated proof");
    assert!(!result.success());
    assert!(!marker.exists());
    assert_eq!(
        fs::read_to_string(env_file).expect("authority reset"),
        "cache_available=false\nverified=false\nerror=SOURCE_VERIFICATION_FAILED\nsourceidentity=literal-source-key\n"
    );
}

#[test]
fn source_record_binds_full_marked_bytes_and_literal_arguments() {
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let record = source_record(
        &[],
        &catalog,
        velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
        "/pinned/node",
        "literal-source-key",
        "test",
    )
    .expect("record");
    assert_eq!(
        record.invocation().descriptor().source_sha256(),
        velnor_actions_contract::compiled_source_sha256(record.source().as_bytes())
    );
    assert_eq!(record.invocation().args()[0], "/pinned/node");
    assert_eq!(record.invocation().args()[1], "literal-source-key");
    assert_eq!(
        record.invocation().args()[2],
        record.invocation().descriptor().source_sha256()
    );
    assert!(record.source().contains("/usr/bin/python3 -I"));
    assert!(record.source().contains("/usr/bin/env -i"));
    assert_eq!(
        record.invocation().args()[3],
        serde_json::to_string(&super::owner_expectation(&catalog)).expect("owner expectation")
    );
    assert_eq!(
        record.invocation().installed_selectors(),
        catalog
            .native_tool_specs(
                velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
                &[velnor_actions_mise::PinnedTool::Node],
            )
            .expect("qualified selectors")
    );
}
