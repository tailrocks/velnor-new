//! T27 failure/security pins for the tofu spawn path (§10 items 1/2/4/5).
//!
//! Pure classification pins (no subprocesses; real runs live in
//! `impl_mise_tofu_t27_realbin`). Item mapping: (1) non-JSON honesty,
//! (2) provider/network failure shape, (4) private registry without
//! credentials, (5) poisoned caches/reports. Item (3) signals and
//! item (6) smoke run the real binary, so they live there.
use std::ffi::OsString;
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::cache_trust::{
    headroom_bytes, parse_service_usage, summarize_cache_usage,
};
use velnor_actions_mise::command::{
    EnvPolicy, IsolatedCommand, is_cancel_or_timeout, is_reserved_env_key,
};
use velnor_actions_mise::restore_evidence::{
    RestoreObservation, classify_restore, verify_provider_restore,
};
use velnor_actions_mise::{MiseError, ProcessOutput};

/// Observed provider-cache entry: real path, bytes, matching digests.
fn provider_cache_observation() -> RestoreObservation {
    let bytes = b"provider package bytes".to_vec();
    RestoreObservation {
        entry_path: "tofu-cache/b3-0000000000000000000000000000000000000000000000000000000000000000/registry.opentofu.org/hashicorp/null".to_owned(),
        entry_bytes: bytes.clone(),
        expected_digest: digest_b3(&bytes),
        expected_compat: digest_b3(b"tofu-1.13.1"),
        observed_compat: digest_b3(b"tofu-1.13.1"),
        expected_owner: "trusted".to_owned(),
        observed_owner: "trusted".to_owned(),
        expected_inputs: digest_b3(b"lock-inputs"),
        observed_inputs: digest_b3(b"lock-inputs"),
    }
}

/// Tofu ctor with the pinned spec for env-shape pins.
fn tofu_command() -> Result<IsolatedCommand, String> {
    IsolatedCommand::tofu_exec(
        &["opentofu@1.13.1".to_owned()],
        &[OsString::from("tofu"), OsString::from("version")],
        "/velnor/tofu-data",
        "/velnor/tofu-cli.hcl",
        "/velnor/tofu-cache",
    )
    .map_err(|err| err.to_string())
}

/// (1) Exit status is authoritative; non-JSON stdout never mints success.
///
/// Inspection: the mise/tofu path parses NO command-output JSON. Tofu
/// `parser_json` walks config files only, and mise `parse_service_usage`
/// scans `gh` service data only. So stdout bytes can neither veto nor
/// grant success (S9: a parse failure is a generic error, never success —
/// here there is no parse step to fail, only the exit gate).
#[test]
fn tofu_exit_status_authoritative_over_non_json_stdout() {
    let failed = ProcessOutput {
        stdout: b"early non-json garbage {{{".to_vec(),
        stderr: b"Error: Missing required provider".to_vec(),
        code: Some(1),
        signal: None,
        success: false,
    };
    assert!(matches!(
        failed.require_success("mise"),
        Err(MiseError::NonZeroExit { code: Some(1), .. })
    ));
    let passed = ProcessOutput {
        stdout: b"early non-json garbage {{{".to_vec(),
        stderr: Vec::new(),
        code: Some(0),
        signal: None,
        success: true,
    };
    assert!(passed.require_success("mise").is_ok());
    assert_eq!(
        passed.stdout_text("mise"),
        Ok("early non-json garbage {{{".to_owned()),
        "raw bytes pass through verbatim; no JSON sniffing"
    );
}

/// (1) Invalid UTF-8 fails early with a typed stream error.
#[test]
fn tofu_stdout_text_rejects_invalid_utf8_early() {
    let output = ProcessOutput {
        stdout: vec![0xff, 0xfe, 0x00],
        stderr: Vec::new(),
        code: Some(0),
        signal: None,
        success: true,
    };
    assert!(matches!(
        output.stdout_text("mise"),
        Err(MiseError::InvalidUtf8 { stream, .. }) if stream == "stdout"
    ));
}

/// (2) Registry/network stderr classifies typed: no success, code kept.
///
/// Shape mirrors the real dead-mirror run (`connection refused`, exit 1):
/// the failure is a `NonZeroExit` outcome, never an abortion, and the
/// stderr survives verbatim for the S4-style remediation match.
#[test]
fn unavailable_provider_stderr_classifies_typed() {
    let stderr = "Error: Failed to resolve provider packages\n\nCould not resolve \
         provider hashicorp/null: failed to query provider mirror \
         https://127.0.0.1:1/ for registry.opentofu.org/hashicorp/null: dial \
         tcp 127.0.0.1:1: connect: connection refused\n";
    let output = ProcessOutput {
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
        code: Some(1),
        signal: None,
        success: false,
    };
    assert!(!output.success);
    let error = output
        .require_success("mise")
        .expect_err("exit 1 must fail");
    assert!(
        matches!(&error, MiseError::NonZeroExit { code: Some(1), stderr: kept, .. }
            if kept == stderr),
        "code and stderr preserved, got {error}"
    );
    assert!(
        !is_cancel_or_timeout(&error),
        "network failure is an outcome, never an abortion: {error}"
    );
}

/// (2) The typed render preserves code and stderr (no swallowing).
#[test]
fn nonzero_exit_display_preserves_code_and_stderr() {
    let error = MiseError::NonZeroExit {
        program: "mise".to_owned(),
        code: Some(1),
        stderr: "connection refused".to_owned(),
    };
    assert_eq!(
        error.to_string(),
        "nonzero_exit: mise: Some(1): connection refused"
    );
}

/// (4) The tofu child env strips every credential/registry-shaped key.
///
/// Hostile ambient values (registry tokens, registry selectors, cloud
/// `*_TOKEN`s, endpoint reroutes, `TF_VAR_*`, checkpoint flags) never
/// reach the tofu child: without credentials a private-registry init
/// fails closed as a typed exit, and there is nothing to leak because
/// nothing arrives. The five baked pairs survive with exact values.
#[test]
fn tofu_child_env_strips_registry_and_credential_keys() -> Result<(), String> {
    let hostile = [
        ("TF_TOKEN_app_terraform_io", "secret-canary"),
        ("TF_REGISTRY_CLIENT_TIMEOUT", "1"),
        ("TF_REGISTRY_DISCOVERY_RETRY", "0"),
        ("TF_VAR_db_password", "secret-canary"),
        ("TF_WORKSPACE", "evil"),
        ("TF_LOG", "TRACE"),
        ("CHECKPOINT_DISABLE", "0"),
        ("GITHUB_TOKEN", "secret-canary"),
        ("GH_TOKEN", "secret-canary"),
        ("CARGO_REGISTRY_TOKEN", "secret-canary"),
        ("CARGO_REGISTRIES_FOO_TOKEN", "secret-canary"),
        ("AWS_SESSION_TOKEN", "secret-canary"),
        ("SOME_TOKEN", "secret-canary"),
        ("GH_HOST", "evil.example"),
        ("GH_CONFIG_DIR", "/evil"),
        ("PATH", "/usr/bin:/bin"),
    ];
    let parent: Vec<(OsString, OsString)> = hostile
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect();
    let command = tofu_command()?;
    let child = command.spawn_env(&parent);
    let texts: Vec<(String, String)> = child
        .iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    for (key, value) in &texts {
        assert!(!value.contains("secret-canary"), "canary leaked via {key}");
        let baked = [
            "TF_IN_AUTOMATION",
            "TF_INPUT",
            "TF_DATA_DIR",
            "TF_CLI_CONFIG_FILE",
            "TF_PLUGIN_CACHE_DIR",
        ];
        assert!(
            !key.starts_with("TF_") || baked.contains(&key.as_str()),
            "ambient TF_ key survived: {key}"
        );
    }
    for key in [
        "TF_TOKEN_app_terraform_io",
        "TF_REGISTRY_CLIENT_TIMEOUT",
        "TF_VAR_db_password",
        "CHECKPOINT_DISABLE",
        "GITHUB_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
        "SOME_TOKEN",
    ] {
        assert!(
            texts.iter().all(|(kept, _)| kept != key),
            "{key} must strip"
        );
    }
    for (key, value) in [
        ("TF_DATA_DIR", "/velnor/tofu-data"),
        ("TF_CLI_CONFIG_FILE", "/velnor/tofu-cli.hcl"),
        ("TF_PLUGIN_CACHE_DIR", "/velnor/tofu-cache"),
        ("TF_IN_AUTOMATION", "1"),
        ("TF_INPUT", "0"),
    ] {
        assert!(
            texts.contains(&(key.to_owned(), value.to_owned())),
            "baked {key}={value} must survive"
        );
    }
    Ok(())
}

/// (4) Credential-shaped extras fail loud; benign extras pass.
///
/// Callers cannot smuggle registry credentials through `with_env`:
/// every `TF_*`/`*_TOKEN`/endpoint spelling is reserved. Honest edge:
/// `TF_REGISTRY_CLIENT_TIMEOUT` is a timeout knob, not a secret, but
/// the prefix rule rejects it too — no carve-outs, fail loud always.
#[test]
fn tofu_extras_reject_credential_shaped_keys_loud() -> Result<(), String> {
    for key in [
        "TF_TOKEN_app_terraform_io",
        "TF_REGISTRY_CLIENT_TIMEOUT",
        "TF_VAR_db_password",
        "TF_WORKSPACE",
        "CHECKPOINT_SYNC_URL",
        "GITHUB_TOKEN",
        "CARGO_REGISTRIES_FOO_TOKEN",
        "DEPLOY_TOKEN",
        "GH_HOST",
    ] {
        assert!(is_reserved_env_key(key), "{key} reserved");
        let hostile = [(OsString::from(key), OsString::from("x"))];
        assert!(
            matches!(
                tofu_command()?.with_env(&hostile),
                Err(MiseError::InvalidStepInput { value, .. })
                if value == "reserved_env_key"
            ),
            "{key} override must fail loud"
        );
    }
    let benign = [(OsString::from("VELNOR_T27_MARKER"), OsString::from("1"))];
    let full = tofu_command()?
        .with_env(&benign)
        .map_err(|err| err.to_string())?
        .full_env();
    assert!(
        full.contains(&benign[0]),
        "benign extras still pass: mechanism discriminates"
    );
    Ok(())
}

/// (4) The tofu Verify policy holds zero credentials by construction.
#[test]
fn tofu_verify_policy_holds_no_credentials() -> Result<(), String> {
    assert_eq!(EnvPolicy::Verify.allowed_credentials(), &[] as &[&str]);
    assert!(
        format!("{:?}", tofu_command()?).contains("Verify"),
        "tofu ctor stays on the credential-free policy"
    );
    Ok(())
}

/// (5) Cross-trust provider-cache entries are rejected, never restored.
///
/// A provider entry restored from another trust scope fails the
/// 5-check chain at `trust_scope_mismatch` (fork-PR isolation: foreign
/// bytes never become local providers).
#[test]
fn cross_trust_provider_cache_entry_rejected() {
    let ok = provider_cache_observation();
    assert_eq!(verify_provider_restore(&ok), Ok(()));
    let mut foreign = ok;
    foreign.observed_owner = "untrusted".to_owned();
    assert_eq!(
        verify_provider_restore(&foreign),
        Err("trust_scope_mismatch")
    );
    assert_eq!(classify_restore(&foreign), Err("trust_scope_mismatch"));
}

/// (5) Poisoned provider-cache bytes fail with precise reasons.
///
/// Tampered bytes, poisoned input digests, and missing entries each
/// fail the model-only 5-check chain at their own reason; a miss
/// would discard the entry for refetch (no live-path consumer yet).
#[test]
fn poisoned_provider_cache_bytes_rejected() {
    let mut tampered = provider_cache_observation();
    tampered.entry_bytes = b"poisoned provider bytes".to_vec();
    assert_eq!(verify_provider_restore(&tampered), Err("cache_corrupt"));
    let mut inputs = provider_cache_observation();
    inputs.observed_inputs = digest_b3(b"forged-inputs");
    assert_eq!(
        verify_provider_restore(&inputs),
        Err("input_digest_mismatch")
    );
    let mut missing = provider_cache_observation();
    missing.entry_path.clear();
    assert_eq!(verify_provider_restore(&missing), Err("no_entry"));
}

/// (5) Poisoned cache-service reports fail closed, never wrap.
///
/// A non-array body is a typed `Contract` rejection; an over-quota
/// body is a typed `InvalidStepInput` rejection — the report never
/// prints a wrapped headroom from attacker-influenced figures.
#[test]
fn poisoned_cache_report_fails_closed() {
    assert!(matches!(
        parse_service_usage("not json at all"),
        Err(MiseError::Contract { .. })
    ));
    assert!(matches!(
        headroom_bytes(200, 100),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "cache_quota"
    ));
    assert!(matches!(
        summarize_cache_usage(r#"[{"sizeInBytes": 200}]"#, 100, 10, 2),
        Err(MiseError::InvalidStepInput { .. })
    ));
    let report = summarize_cache_usage(r#"[{"sizeInBytes": 60}]"#, 100, 10, 2)
        .expect("in-quota report builds");
    assert_eq!(report.active_bytes, 60);
    assert_eq!(report.headroom_bytes, 40);
    assert_eq!(report.aggregate_transfer_bytes, 20);
}
