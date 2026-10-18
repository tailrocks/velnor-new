use std::path::PathBuf;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, MiseTaskSource,
    RustBinaryPreviewProducer, VerificationRunner,
};
use velnor_actions_mise as mise;
use velnor_actions_mise::checks::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> TestResult<Self> {
        let root = crate::test_temp_dir::unique_temp_dir("velnor-check-test")?;
        let fixture = Self(root);
        std::fs::write(fixture.0.join("mise.toml"), source)?;
        Ok(fixture)
    }
    fn discover(&self, task: &str) -> Result<Vec<DiscoveredCheck>, mise::MiseError> {
        self.discover_tools(task, vec![])
    }
    fn discover_tools(
        &self,
        task: &str,
        tools: Vec<String>,
    ) -> Result<Vec<DiscoveredCheck>, mise::MiseError> {
        let row = MiseCheck {
            id: "native".into(),
            task: task.into(),
            directory: ".".into(),
            runner: CheckRunner {
                label: "macos-14".into(),
                platform: CheckPlatform::MacosArm64,
                executor: CheckExecutor::Hosted,
                container: None,
            },
            inputs: vec![],
            tools,
            system_tools: vec![],
            evidence: None,
            timeout_minutes: 30,
        };
        discover_checks(&self.0, &[row], &[])
    }
}

#[test]
fn catalog_names_do_not_authorize_named_check_tools() -> TestResult {
    let root = Fixture::new("[tasks.probe]\nrun='true'\n")?;
    assert!(root.discover_tools("probe", vec!["rust".into()]).is_err());
    assert!(
        root.discover_tools("probe", vec!["cargo-nextest".into()])
            .is_err()
    );
    Ok(())
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("velnor_fixture_cleanup_failed:{:?}", error.kind());
        }
    }
}

#[test]
fn static_discovery_never_runs_poison_and_retains_all_native_tasks() -> TestResult {
    let root = Fixture::new(
        "[env]\nPOISON='bad'\n[hooks]\nenter='touch POISON'\n[tools]\n'cargo:poison'='latest'\n[tasks.dep]\nrun='touch NOT_RUN'\n[tasks.probe]\ndepends=['dep']\nrun='mise run nested'\n[tasks.nested]\nrun='true'\n",
    )?;
    let check = root.discover("probe")?.remove(0);
    assert!(!root.0.join("NOT_RUN").exists());
    assert!(!root.0.join("POISON").exists());
    assert!(!check.task_config.contains("[tools]"));
    assert!(!check.task_config.contains("[env]"));
    assert!(!check.task_config.contains("[hooks]"));
    assert!(check.task_config.contains("[tasks.nested]"));
    assert!(check.proposal.identity.undeclared_reads);
    assert!(!check.proposal.cache_policy.allow_task_reuse);
    assert!(!check.proposal.cache_policy.allow_compilation_reuse);
    assert_eq!(
        check.proposal.payload,
        vec![std::ffi::OsString::from("probe")]
    );
    assert!(check.config_inputs.contains(&"mise.toml".to_owned()));
    Ok(())
}

#[test]
fn missing_task_and_dependency_fail_before_execution() -> TestResult {
    assert!(
        Fixture::new("[tasks.other]\nrun='true'\n")?
            .discover("missing")
            .is_err()
    );
    assert!(
        Fixture::new("[tasks.probe]\ndepends=['missing']\nrun='true'\n")?
            .discover("probe")
            .is_err()
    );
    Ok(())
}

#[test]
fn unsupported_config_authority_fails_explicitly() -> TestResult {
    for source in [
        "includes=['other.toml']\n[tasks.probe]\nrun='true'\n",
        "[tasks.probe]\nrun='true'\ntools={rust='nightly'}\n",
        "[tasks.probe]\nrun='true'\nenv={GH_TOKEN='poison'}\n",
        "[tasks.probe]\nrun='{{exec(command=\"touch POISON\")}}'\n",
    ] {
        assert!(Fixture::new(source)?.discover("probe").is_err(), "{source}");
    }
    Ok(())
}

#[test]
fn repository_inputs_reject_escape_and_symlink_escape() -> TestResult {
    let root = Fixture::new("[tasks.probe]\nrun='true'\n")?;
    assert!(repository_path(&root.0, "../mise.toml").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(std::env::temp_dir(), root.0.join("outside"))?;
        assert!(repository_path(&root.0, "outside").is_err());
    }
    Ok(())
}

#[test]
fn oversized_named_check_source_is_rejected_before_parsing() -> TestResult {
    let root = Fixture::new("[tasks.probe]\nrun='true'\n")?;
    let oversized = vec![b'x'; velnor_actions_contract::MAX_CHECK_SOURCE_BYTES + 1];
    std::fs::write(root.0.join("mise.toml"), oversized)?;
    assert!(root.discover("probe").is_err());
    Ok(())
}

#[test]
fn projection_binds_cwd_and_removes_nested_freshness() -> TestResult {
    let root = Fixture::new(
        "[tasks.probe]\nsources=['src/**']\noutputs=['output']\nrun='''\necho hello\n[tasks.fake]\n'''\n",
    )?;
    let check = root.discover("probe")?.remove(0);
    let owned = QualifiedCheck::new(
        std::env::temp_dir().join("isolated-projection-test"),
        root.0.canonicalize()?,
        check,
        CheckCapabilityProof { container: None },
        &[],
    )?;
    let projection = owned.bound_projection()?;
    assert!(!projection.contains("sources="));
    assert!(!projection.contains("outputs="));
    assert_eq!(projection.matches("dir = ").count(), 1);
    assert!(projection.contains("[tasks.fake]"));
    Ok(())
}

#[test]
fn full_source_bytes_remain_bound_after_task_projection() -> TestResult {
    let source = "[env]\nPOISON='bad'\n[tasks.probe]\nrun='true'\n";
    let root = Fixture::new(source)?;
    let check = root.discover("probe")?.remove(0);
    assert_eq!(check.config_source, source);
    assert!(!check.task_config.contains("[env]"));
    Ok(())
}

#[test]
fn preview_producer_binds_explicit_nested_source_and_sanitizes_task_projection() -> TestResult {
    let root = Fixture::new("[tasks.wrong]\nrun='touch WRONG_SOURCE'\n")?;
    let nested = root.0.join("native/apple");
    std::fs::create_dir_all(&nested)?;
    std::fs::write(root.0.join("native/Cargo.toml"), "[package]\nname='x'\n")?;
    let source = "[env]\nPOISON='bad'\n[hooks]\nenter='touch POISON'\n[tools]\nrust='nightly'\n[tasks.helper]\nrun='true'\n[tasks.bundle-preview]\ndepends=['helper']\nrun='true'\n";
    std::fs::write(root.0.join("native/mise.toml"), source)?;
    let producer = RustBinaryPreviewProducer {
        task: "bundle-preview".to_owned(),
        source: MiseTaskSource {
            mise_config: "native/mise.toml".to_owned(),
            working_directory: "native/apple".to_owned(),
        },
        runner: VerificationRunner::Macos26Arm64,
        inputs: vec!["native/Cargo.toml".to_owned()],
        ..RustBinaryPreviewProducer::default()
    };
    let discovered = discover_preview_producer(
        &root.0,
        &producer,
        &[],
        mise::CheckDeadline::after(std::time::Duration::from_secs(30))?,
    )?;

    assert_eq!(discovered.check.directory, "native/apple");
    assert_eq!(discovered.check.runner.label, "macos-26");
    assert_eq!(discovered.check.runner.platform, CheckPlatform::MacosArm64);
    assert_eq!(discovered.proposal.identity.unit_path, "native/mise.toml");
    assert_eq!(discovered.config_source, source);
    assert!(
        discovered
            .config_inputs
            .contains(&"native/mise.toml".to_owned())
    );
    assert!(
        discovered
            .config_inputs
            .contains(&"native/Cargo.toml".to_owned())
    );
    assert!(discovered.task_config.contains("[tasks.bundle-preview]"));
    assert!(discovered.task_config.contains("[tasks.helper]"));
    assert!(!discovered.task_config.contains("[env]"));
    assert!(!discovered.task_config.contains("[hooks]"));
    assert!(!discovered.task_config.contains("[tools]"));
    assert!(!root.0.join("POISON").exists());
    assert!(!root.0.join("WRONG_SOURCE").exists());

    let nested_check = MiseCheck {
        id: "native".into(),
        task: "bundle-preview".into(),
        directory: "native/apple".into(),
        runner: CheckRunner {
            label: "macos-26".into(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec![],
        tools: vec![],
        system_tools: vec![],
        evidence: None,
        timeout_minutes: 30,
    };
    assert!(discover_checks(&root.0, &[nested_check], &[]).is_err());
    Ok(())
}

#[test]
fn preview_producer_rejects_includes_and_file_task_config_authority() -> TestResult {
    let root = Fixture::new("[tasks.placeholder]\nrun='true'\n")?;
    let producer = RustBinaryPreviewProducer {
        task: "bundle-preview".to_owned(),
        source: MiseTaskSource::default(),
        inputs: vec!["Cargo.lock".to_owned()],
        ..RustBinaryPreviewProducer::default()
    };
    for source in [
        "includes=['other.toml']\n[tasks.bundle-preview]\nrun='true'\n",
        "task_config='tasks'\n[tasks.bundle-preview]\nrun='true'\n",
        "task_dir='tasks'\n[tasks.bundle-preview]\nrun='true'\n",
        "task_dirs=['tasks']\n[tasks.bundle-preview]\nrun='true'\n",
    ] {
        std::fs::write(root.0.join("mise.toml"), source)?;
        assert!(
            discover_preview_producer(
                &root.0,
                &producer,
                &[],
                mise::CheckDeadline::after(std::time::Duration::from_secs(30))?,
            )
            .is_err(),
            "source authority accepted: {source}"
        );
    }
    Ok(())
}

#[test]
fn opaque_metadata_serializes_one_fixed_disabled_reuse_policy() -> TestResult {
    let root = Fixture::new("[tasks.probe]\nrun='true'\n")?;
    let check = root.discover("probe")?.remove(0);
    let metadata = serde_json::to_value(check.entry_metadata())?;
    assert_eq!(metadata["opaque"], true);
    for key in [
        "allow_compilation_reuse",
        "allow_task_reuse",
        "task_cache_enabled",
        "artifact_cache_enabled",
    ] {
        assert_eq!(metadata[key], false, "{key}");
    }
    assert!(metadata.get("policy").is_none());
    assert_eq!(metadata["system_tools"], serde_json::json!([]));
    Ok(())
}

#[test]
fn qualified_environment_clears_credential_and_config_poison() -> TestResult {
    use mise::command::EnvPolicy;
    let parent = [
        "GITHUB_TOKEN",
        "SECRET_TOKEN",
        "MISE_CONFIG_FILE",
        "MISE_OVERRIDE_CONFIG_FILENAMES",
        "HOME",
        "PATH",
    ]
    .iter()
    .map(|k| {
        (
            std::ffi::OsString::from(k),
            std::ffi::OsString::from("poison"),
        )
    })
    .collect::<Vec<_>>();
    assert_eq!(EnvPolicy::QualifiedCheck.child_env(&parent, &[]).len(), 0);
    let ordinary = mise::IsolatedCommand::repo_task("true", vec![], &[])?;
    assert!(
        ordinary
            .full_env()
            .iter()
            .any(|(key, value)| key == "MISE_NO_CONFIG" && value == "1")
    );
    Ok(())
}
