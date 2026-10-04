use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, ExitStatus};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::StepKind;

use super::{
    MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_RESTORE_NAME,
    QualificationObserverBinding, qualification_observer_steps, step_yaml_id,
};
use crate::cache_steps::{TOOLS_RESTORE_USES, TOOLS_SAVE_USES};

#[path = "mbx_bundle_observer_import_tests.rs"]
mod import_tests;

const SCOPE: &str = "qualification-mbx-v1/cancel-pre-save-victim";
const VERSION: &str = "1.21.1";
const GENERATION: &str = "velnor-mbx-1.21.1";
const SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
const WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main";
const RUSTC_IDENTITY: &str = "rustc 1.98.1\ncommit-hash: observer-test\n";

#[test]
fn observer_lifecycle_is_exact_read_only_and_reserved() -> Result<(), String> {
    let case = valid_case()?;
    let steps = observer_steps(&case)?;
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].name, MBX_BUNDLE_KEY_NAME);
    assert_eq!(step_yaml_id(&steps[0]), Some("mbx-bundle-key"));
    assert_eq!(steps[1].name, MBX_BUNDLE_RESTORE_NAME);
    assert_eq!(steps[2].name, MBX_BUNDLE_IMPORT_NAME);
    assert_eq!(step_yaml_id(&steps[2]), Some("mbx-bundle-import"));

    let StepKind::Action { uses, with, .. } = &steps[1].kind else {
        return Err("observer restore is not an action".to_owned());
    };
    assert_eq!(uses, TOOLS_RESTORE_USES);
    assert_eq!(
        with.get("key").map(String::as_str),
        Some("${{ steps.mbx-bundle-key.outputs.primary }}")
    );
    assert!(!with.contains_key("restore-keys"));
    assert!(steps.iter().all(|step| {
        !matches!(&step.kind, StepKind::Action { uses, .. } if uses == TOOLS_SAVE_USES)
    }));
    assert!(steps.iter().all(|step| {
        step.condition.as_deref().is_some_and(|condition| {
            condition.contains("github.event_name == 'workflow_dispatch'")
                && condition.contains("github.ref == 'refs/heads/main'")
                && condition.contains("github.ref_protected == true")
                && condition.contains("steps.mbx-cancel-receipt.outputs.should_observe == 'true'")
        })
    }));
    let bad_identity = "a".repeat(64);
    let binding = QualificationObserverBinding {
        child_run_id: "9",
        child_attempt: "1",
        source_sha: SHA,
        receipt_primary: "key",
        receipt_generation: GENERATION,
        receipt_rustc_identity: &bad_identity,
        receipt_version: VERSION,
    };
    assert!(
        qualification_observer_steps("ordinary-cache", VERSION, &rust_env(), &binding).is_err()
    );
    Ok(())
}

#[test]
fn observer_key_derives_child_identity_and_rejects_mismatch() -> Result<(), String> {
    let valid = valid_case()?;
    let output = run_key("valid", &valid)?;
    assert!(output.status.success(), "{}", output.stderr);
    assert_eq!(
        output.value("primary"),
        Some(valid.receipt_primary.as_str())
    );
    assert_eq!(output.value("generation"), Some(GENERATION));
    assert_eq!(
        output.value("rustc_identity"),
        Some(valid.receipt_rustc_identity.as_str())
    );
    assert_eq!(output.value("mbx_version"), Some(VERSION));
    assert!(!output.values.contains("pr-cache-allowed="));

    for hostile in hostile_cases(&valid) {
        let output = run_key(&hostile.label, &hostile.case)?;
        assert!(
            !output.status.success(),
            "{} unexpectedly passed",
            hostile.label
        );
    }
    Ok(())
}

fn observer_steps(case: &ObserverCase) -> Result<Vec<velnor_actions_contract::Step>, String> {
    let binding = QualificationObserverBinding {
        child_run_id: &case.run_id,
        child_attempt: &case.attempt,
        source_sha: &case.source_sha,
        receipt_primary: &case.receipt_primary,
        receipt_generation: &case.receipt_generation,
        receipt_rustc_identity: &case.receipt_rustc_identity,
        receipt_version: &case.receipt_version,
    };
    qualification_observer_steps(SCOPE, VERSION, &rust_env(), &binding)
        .map_err(|error| error.to_string())
}

fn valid_case() -> Result<ObserverCase, String> {
    let run_id = "901".to_owned();
    let attempt = "2".to_owned();
    Ok(ObserverCase {
        label: "valid".to_owned(),
        event: "workflow_dispatch".to_owned(),
        reference: "refs/heads/main".to_owned(),
        protected: "true".to_owned(),
        run_id,
        attempt,
        source_sha: SHA.to_owned(),
        receipt_primary: expected_primary("901", "2", SHA)?,
        receipt_generation: GENERATION.to_owned(),
        receipt_rustc_identity: sha256(RUSTC_IDENTITY)?,
        receipt_version: VERSION.to_owned(),
    })
}

fn expected_primary(run_id: &str, attempt: &str, sha: &str) -> Result<String, String> {
    let workflow_path = WORKFLOW_REF
        .split_once('@')
        .map(|(path, _)| path)
        .ok_or_else(|| "bad test workflow ref".to_owned())?;
    let scope_hash = sha256(&format!("{workflow_path}\n{SCOPE}\n{{}}\n"))?;
    let compiler_hash = sha256(RUSTC_IDENTITY)?;
    Ok(format!(
        "linux-x64-mbx-{GENERATION}-dir-rust-1.98.1-{compiler_hash}-scope-{scope_hash}-run-{run_id}-attempt-{attempt}-{sha}"
    ))
}

fn hostile_cases(valid: &ObserverCase) -> Vec<HostileCase> {
    let mut cases = Vec::new();
    for (label, field, value) in [
        ("event-pull-request", "event", "pull_request"),
        ("event-target", "event", "pull_request_target"),
        ("ref-other", "reference", "refs/heads/attacker"),
        ("unprotected", "protected", "false"),
        ("run-zero", "run_id", "0"),
        ("run-leading-zero", "run_id", "0901"),
        ("run-invalid", "run_id", "9x"),
        ("attempt-zero", "attempt", "0"),
        ("attempt-invalid", "attempt", "1x"),
        (
            "sha-upper",
            "source_sha",
            "ABCDEF0123456789ABCDEF0123456789ABCDEF01",
        ),
        ("sha-short", "source_sha", "abcdef"),
        (
            "receipt-generation",
            "receipt_generation",
            "other-generation",
        ),
        ("receipt-rustc", "receipt_rustc_identity", "a"),
        ("receipt-version", "receipt_version", "9.9.9"),
        ("receipt-primary", "receipt_primary", "other-key"),
    ] {
        let mut case = valid.clone();
        case.label = label.to_owned();
        match field {
            "event" => case.event = value.to_owned(),
            "reference" => case.reference = value.to_owned(),
            "protected" => case.protected = value.to_owned(),
            "run_id" => case.run_id = value.to_owned(),
            "attempt" => case.attempt = value.to_owned(),
            "source_sha" => case.source_sha = value.to_owned(),
            "receipt_generation" => case.receipt_generation = value.to_owned(),
            "receipt_rustc_identity" => case.receipt_rustc_identity = value.to_owned(),
            "receipt_version" => case.receipt_version = value.to_owned(),
            "receipt_primary" => case.receipt_primary = value.to_owned(),
            _ => continue,
        }
        cases.push(HostileCase {
            label: label.to_owned(),
            case,
        });
    }
    cases
}

fn run_key(label: &str, case: &ObserverCase) -> Result<KeyOutput, String> {
    let scratch = Scratch::new(label)?;
    let bin = scratch.0.join("bin");
    fs::create_dir_all(&bin).map_err(|error| io_error(&error))?;
    executable(
        &bin.join("mise"),
        "#!/bin/sh\nprintf 'rustc 1.98.1\\ncommit-hash: observer-test\\n'\n",
    )?;
    let github_output = scratch.0.join("github-output");
    fs::write(&github_output, "").map_err(|error| io_error(&error))?;
    let steps = observer_steps(case)?;
    let StepKind::Shell { run, env } = &steps[0].kind else {
        return Err("observer key is not shell".to_owned());
    };
    let script = run
        .get(2)
        .ok_or_else(|| "observer key script missing".to_owned())?;
    let mut command_env = env.clone();
    command_env.extend(BTreeMap::from([
        ("MBX_VERSION".to_owned(), VERSION.to_owned()),
        ("MBX_MATRIX_CONTEXT".to_owned(), "{}".to_owned()),
        (
            "GITHUB_OUTPUT".to_owned(),
            github_output.display().to_string(),
        ),
        ("GITHUB_WORKFLOW_REF".to_owned(), WORKFLOW_REF.to_owned()),
        ("GITHUB_SHA".to_owned(), case.source_sha.clone()),
        ("GITHUB_EVENT_NAME".to_owned(), case.event.clone()),
        ("GITHUB_REF".to_owned(), case.reference.clone()),
        ("GITHUB_REF_PROTECTED".to_owned(), case.protected.clone()),
        ("RUNNER_OS".to_owned(), "Linux".to_owned()),
        ("RUNNER_ARCH".to_owned(), "X64".to_owned()),
        (
            "PATH".to_owned(),
            format!("{}:/usr/bin:/bin:/sbin:/opt/homebrew/bin", bin.display()),
        ),
    ]));
    let result = execute(script, command_env)?;
    let values = fs::read_to_string(&github_output).map_err(|error| io_error(&error))?;
    Ok(KeyOutput {
        status: result.status,
        stderr: result.stderr,
        values,
        _scratch: scratch,
    })
}

fn execute(script: &str, env: BTreeMap<String, String>) -> Result<RunOutput, String> {
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .env_clear()
        .envs(env)
        .output()
        .map_err(|error| io_error(&error))?;
    Ok(RunOutput {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn rust_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
        ("MISE_RUSTUP_HOME".to_owned(), "/tmp/rustup".to_owned()),
        ("MISE_CARGO_HOME".to_owned(), "/tmp/cargo".to_owned()),
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
    ])
}

fn sha256(value: &str) -> Result<String, String> {
    let result = Command::new("/bin/bash")
        .arg("-c")
        .arg("printf '%s' \"$HASH_VALUE\" | sha256sum | cut -c1-64")
        .env_clear()
        .env("HASH_VALUE", value)
        .env("PATH", "/usr/bin:/bin:/sbin:/opt/homebrew/bin")
        .output()
        .map_err(|error| io_error(&error))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn executable(path: &std::path::Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| io_error(&error))?;
    let mut permissions = fs::metadata(path)
        .map_err(|error| io_error(&error))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|error| io_error(&error))
}

fn io_error(error: &std::io::Error) -> String {
    error.to_string()
}

struct ObserverCase {
    label: String,
    event: String,
    reference: String,
    protected: String,
    run_id: String,
    attempt: String,
    source_sha: String,
    receipt_primary: String,
    receipt_generation: String,
    receipt_rustc_identity: String,
    receipt_version: String,
}

impl Clone for ObserverCase {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            event: self.event.clone(),
            reference: self.reference.clone(),
            protected: self.protected.clone(),
            run_id: self.run_id.clone(),
            attempt: self.attempt.clone(),
            source_sha: self.source_sha.clone(),
            receipt_primary: self.receipt_primary.clone(),
            receipt_generation: self.receipt_generation.clone(),
            receipt_rustc_identity: self.receipt_rustc_identity.clone(),
            receipt_version: self.receipt_version.clone(),
        }
    }
}

struct HostileCase {
    label: String,
    case: ObserverCase,
}

struct KeyOutput {
    status: ExitStatus,
    stderr: String,
    values: String,
    _scratch: Scratch,
}

impl KeyOutput {
    fn value(&self, key: &str) -> Option<&str> {
        self.values.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name == key).then_some(value)
        })
    }
}

struct RunOutput {
    status: ExitStatus,
    stderr: String,
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-mbx-observer-{}-{label}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|error| io_error(&error))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not clean MBX observer test directory: {error}");
        }
    }
}
