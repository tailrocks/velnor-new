use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, ResourcesConfig, StacksConfig, TestShardingConfig,
    WorkflowConfig, WorkflowPolicy,
};

/// Config carrying exactly the given action-pin overrides.
fn config_with(overrides: BTreeMap<String, ActionPinOverride>) -> VelnorConfig {
    VelnorConfig {
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
        },
        resources: ResourcesConfig {
            compiler_process_budget: 2,
            test_process_budget: 2,
        },
        test_sharding: TestShardingConfig {
            default_shards: 1,
            by_manifest: BTreeMap::new(),
        },
        stacks: StacksConfig {
            ignore: Vec::new(),
            rust: None,
            tofu: None,
        },
        discovery: DiscoveryConfig {
            exclude: Vec::new(),
        },
        actions: ActionsConfig { overrides },
        execution: None,
    }
}

#[test]
fn source_build_consumer_generation_fails_with_provenance() {
    let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", None);
    assert!(err.is_err_and(|err| {
        err.to_string()
            .contains("consumer_requires_release_install")
    }));
}

#[test]
fn consumer_manifest_mismatch_and_bad_target_fail() {
    let err = consumer_acquire_from("ubuntu-26.04", "9.9.9", Some(&test_manifest_json()));
    assert!(err.is_err_and(|err| err.to_string().contains("version_mismatch")));
    let err = consumer_acquire_from(
        "ubuntu-26.04-arm",
        env!("CARGO_PKG_VERSION"),
        Some(&test_manifest_json()),
    );
    assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), Some("not json"));
    assert!(err.is_err());
}

#[test]
fn consumer_gate_rejects_attacker_manifests() {
    let version = env!("CARGO_PKG_VERSION");
    let sha = "a".repeat(64);
    let manifest = |repository: &str, artifact: &str| {
        format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"{repository}\",\"commit\":\"{}\",\"targets\":[{{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"{artifact}\",\"sha256\":\"{sha}\"}}]}}",
            "a".repeat(40)
        )
    };
    let bound = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-x86_64-unknown-linux-gnu"
    );
    for (repository, artifact) in [
        ("evil/velnor-new", bound.as_str()),
        (
            "tailrocks/velnor-new",
            "https://evil.example/r/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        ),
        ("tailrocks/velnor-new", "https://github.com@evil.example/x"),
    ] {
        let json = manifest(repository, artifact);
        assert!(
            consumer_acquire_from("ubuntu-26.04", version, Some(&json)).is_err(),
            "accepted {repository} {artifact}"
        );
    }
}

#[test]
fn consumer_manifest_without_commit_fails() {
    let full = test_manifest_json();
    let segment = format!("\"commit\":\"{}\",", "a".repeat(40));
    assert!(full.contains(&segment), "fixture must carry commit");
    let missing = full.replace(&segment, "");
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), Some(&missing))
        .expect_err("commit required");
    assert!(err.to_string().contains("commit"), "{err}");
    let malformed = full.replace(&segment, "\"commit\":\"xyz\",");
    let err = consumer_acquire_from("ubuntu-26.04", env!("CARGO_PKG_VERSION"), Some(&malformed))
        .expect_err("malformed commit fails");
    assert!(err.to_string().contains("malformed_commit"), "{err}");
}

#[test]
fn fixture_manifest_embeds_runner_target_record() {
    let step = consumer_acquire_from(
        "ubuntu-26.04",
        env!("CARGO_PKG_VERSION"),
        Some(&test_manifest_json()),
    )
    .map(|step| step.name);
    assert_eq!(
        step.map_err(|err| err.to_string()),
        Ok("Acquire Velnor".to_owned())
    );
}

#[cfg(unix)]
#[test]
fn macos_acquire_uses_shasum_when_sha256sum_is_absent() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::process::Command;
    use velnor_actions_contract::StepKind;

    let version = env!("CARGO_PKG_VERSION");
    let step = consumer_acquire_from("macos-15", version, Some(&test_manifest_json()))?;
    let StepKind::Shell { run, .. } = step.kind else {
        return Err("Acquire Velnor must use a shell step".into());
    };
    let temp = tempfile::tempdir()?;
    let bin = temp.path().join("bin");
    let destination = temp.path().join("velnor").join("bin");
    fs::create_dir(&bin)?;
    fs::create_dir_all(&destination)?;
    let staged = destination.join(format!("velnor-actions-{version}"));
    let sha = "a".repeat(64);
    let call_log = temp.path().join("shasum-call");
    write_executable(&bin.join("mkdir"), "#!/bin/sh\nexit 0\n")?;
    write_executable(
        &bin.join("curl"),
        "#!/bin/sh\nout=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = -o ]; then shift; out=$1; fi\n  shift\ndone\n[ -n \"$out\" ] || exit 40\nprintf payload > \"$out\"\n",
    )?;
    write_executable(
        &bin.join("shasum"),
        "#!/bin/sh\n[ \"$*\" = '-a 256 -c -' ] || exit 41\nIFS= read -r line || exit 42\n[ \"$line\" = \"$TEST_SHA256  $TEST_STAGED\" ] || exit 43\nprintf '%s' \"$*\" > \"$TEST_SHA_CALL\"\n",
    )?;
    write_executable(
        &bin.join("chmod"),
        "#!/bin/sh\n[ \"$1\" = +x ] && [ \"$2\" = \"$TEST_STAGED\" ]\n",
    )?;

    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(&run[2])
        .env_clear()
        .env("PATH", &bin)
        .env("RUNNER_TEMP", temp.path())
        .env("VELNOR_ASSET_URL", "https://example.invalid/asset")
        .env("VELNOR_ASSET_SHA256", &sha)
        .env("TEST_SHA256", &sha)
        .env("TEST_STAGED", &staged)
        .env("TEST_SHA_CALL", &call_log)
        .output()?;
    assert!(
        output.status.success(),
        "script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(call_log)?, "-a 256 -c -");
    assert_eq!(fs::read_to_string(&staged)?, "payload");

    fs::remove_file(bin.join("shasum"))?;
    let missing_verifier = Command::new("/bin/sh")
        .arg("-c")
        .arg(&run[2])
        .env_clear()
        .env("PATH", &bin)
        .env("RUNNER_TEMP", temp.path())
        .env("VELNOR_ASSET_URL", "https://example.invalid/asset")
        .env("VELNOR_ASSET_SHA256", &sha)
        .env("TEST_SHA256", &sha)
        .env("TEST_STAGED", &staged)
        .env("TEST_SHA_CALL", temp.path().join("missing-shasum-call"))
        .output()?;
    assert!(!missing_verifier.status.success());
    assert!(
        String::from_utf8_lossy(&missing_verifier.stderr)
            .contains("no SHA-256 utility is available")
    );
    Ok(())
}

/// Write an executable shell fixture for the isolated acquisition test.
#[cfg(unix)]
fn write_executable(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[test]
fn mise_setup_defaults_to_compiled_pins() {
    let setup = resolve_mise_setup(&config_with(BTreeMap::new()), "ubuntu-26.04")
        .map_err(|err| err.to_string());
    assert_eq!(
        setup,
        Ok(MiseSetup {
            uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
            version: MISE_VERSION.to_owned(),
            sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        })
    );
}

#[test]
fn mise_setup_accepts_approved_override_only() {
    let approved = BTreeMap::from([(
        MISE_ACTION_KEY.to_owned(),
        ActionPinOverride {
            sha: MISE_ACTION_SHA.to_owned(),
            version: MISE_ACTION_VERSION.to_owned(),
        },
    )]);
    let setup = resolve_mise_setup(&config_with(approved), "ubuntu-26.04");
    assert!(setup.is_ok_and(|setup| setup.uses.ends_with(MISE_ACTION_SHA)));
    for pin in [
        ActionPinOverride {
            sha: "0".repeat(40),
            version: MISE_ACTION_VERSION.to_owned(),
        },
        ActionPinOverride {
            sha: MISE_ACTION_SHA.to_owned(),
            version: "v9.9.9".to_owned(),
        },
        ActionPinOverride {
            sha: "short".to_owned(),
            version: MISE_ACTION_VERSION.to_owned(),
        },
    ] {
        let overrides = BTreeMap::from([(MISE_ACTION_KEY.to_owned(), pin)]);
        assert!(resolve_mise_setup(&config_with(overrides), "ubuntu-26.04").is_err());
    }
}

#[test]
fn mise_setup_rejects_non_linux_runners() {
    for label in ["ubuntu-26.04-arm", "windows-latest", "ubuntu-latest"] {
        let err = resolve_mise_setup(&config_with(BTreeMap::new()), label);
        assert!(
            err.is_err_and(|err| err.to_string().contains("mise_setup_unsupported_target")),
            "{label}"
        );
    }
}
