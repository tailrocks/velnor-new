use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, ResourcesConfig, StacksConfig, TestShardingConfig,
    VerificationRunner, WorkflowConfig, WorkflowPolicy,
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
            tasks: Vec::new(),
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
    let err = consumer_acquire_from("ubuntu-26.04-arm", "0.1.0", Some(&test_manifest_json()));
    assert!(err.is_err_and(|err| err.to_string().contains("unsupported_target_for_runner")));
    let err = consumer_acquire_from("ubuntu-26.04", "0.1.0", Some("not json"));
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

#[test]
fn verification_mise_setup_pins_each_runner_architecture() {
    let config = config_with(BTreeMap::new());
    let linux = resolve_verification_mise_setup(&config, VerificationRunner::LinuxX64)
        .expect("Linux Mise pin");
    let macos = resolve_verification_mise_setup(&config, VerificationRunner::MacosArm64)
        .expect("Apple ARM64 Mise pin");

    assert_eq!(linux.version, MISE_VERSION);
    assert_eq!(linux.sha256, MISE_BINARY_SHA256_LINUX_X64);
    assert_eq!(macos.version, MISE_VERSION);
    assert_eq!(macos.sha256, MISE_BINARY_SHA256_MACOS_ARM64);
    assert_ne!(linux.sha256, macos.sha256);
}

#[test]
fn generator_seed_hit_skips_curl_and_a_bad_hash_does_not_copy() {
    let root = std::env::temp_dir().join(format!("velnor-gen-seed-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let name = "velnor-actions-0.1.0";
    let seed_file = root.join("generator").join(name);
    std::fs::create_dir_all(seed_file.parent().expect("parent")).expect("seed dir");
    std::fs::write(&seed_file, b"generator-bytes").expect("seed");
    let staged = root.join("stage").join(name);
    let argv = acquire_script_argv(staged.to_str().expect("utf8"), root.to_str().expect("utf8"))
        .expect("argv");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let curl = bin.join("curl");
    std::fs::write(&curl, "#!/bin/sh\necho \"$@\" >> \"$CURL_LOG\"\nexit 22\n").expect("curl");
    let mut mode = std::fs::metadata(&curl).expect("meta").permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(&curl, mode).expect("mode");
    let hash = file_sha256(&seed_file);
    let hit = run_acquire(&argv[2], &bin, &hash);
    assert!(hit.status.success(), "{hit:?}");
    assert_eq!(std::fs::read(&staged).expect("staged"), b"generator-bytes");
    assert!(read_log(&bin).is_empty(), "{}", read_log(&bin));
    std::fs::remove_file(&staged).expect("clear");
    let miss = run_acquire(&argv[2], &bin, &"a".repeat(64));
    assert!(!miss.status.success(), "bad hash must not succeed");
    assert!(!staged.exists(), "bad seed is not staged");
    assert!(read_log(&bin).contains("https://example.invalid/generator"));
    assert_eq!(std::fs::read(&seed_file).expect("seed"), b"generator-bytes");
    std::fs::remove_dir_all(&root).ok();
}

fn file_sha256(path: &std::path::Path) -> String {
    let output = std::process::Command::new("/sbin/sha256sum")
        .arg(path)
        .output()
        .expect("sha256sum");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("hash")
        .to_owned()
}

fn run_acquire(script: &str, bin: &std::path::Path, hash: &str) -> std::process::Output {
    let path = format!("{}:/sbin:/usr/bin:/bin", bin.display());
    std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("PATH", &path)
        .env("VELNOR_ASSET_SHA256", hash)
        .env("VELNOR_ASSET_URL", "https://example.invalid/generator")
        .env("CURL_LOG", bin.join("curl.log"))
        .output()
        .expect("sh")
}

fn read_log(bin: &std::path::Path) -> String {
    std::fs::read_to_string(bin.join("curl.log")).unwrap_or_default()
}
