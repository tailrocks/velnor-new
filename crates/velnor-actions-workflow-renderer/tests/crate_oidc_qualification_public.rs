// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_workflow_renderer::crate_oidc_qualification::OidcQualificationSpec;
use velnor_actions_workflow_renderer::yaml::{Yaml, render_yaml};

fn spec() -> OidcQualificationSpec {
    OidcQualificationSpec {
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        trusted_reviewer_id: 42,
        generator_version: "0.1.78".into(),
        generator_source_sha: "a".repeat(40),
        generator_manifest_sha256: "b".repeat(64),
    }
}

fn intent() -> velnor_actions_contract::config::CrateReleaseIntent {
    use velnor_actions_contract::config::{CrateReleaseIntent, ReleaseAuthentication};
    CrateReleaseIntent {
        schema: 1,
        oidc_qualification: true,
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        trusted_reviewer_id: 42,
        rust_version: "1.99.1".into(),
        generator_version: "0.1.78".into(),
        generator_source_sha: "a".repeat(40),
        generator_manifest_sha256: "b".repeat(64),
        release_plz_version: "0.4.3".into(),
        runner: "ubuntu-26.04".into(),
        authentication: ReleaseAuthentication::TrustedPublishing,
        manifest_path: "Cargo.toml".into(),
        pr_write_paths: vec!["Cargo.toml".into()],
        registry: "crates-io".into(),
        packages: vec!["example".into()],
        publish_features: [("example".into(), vec![])].into(),
        version_groups: BTreeMap::default(),
        new_crates: None,
        tag_template: "{{ package }}-v{{ version }}".into(),
        recovery: None,
        journal_predecessor: None,
        continuation_source_sha: None,
    }
}

fn field<'a>(value: &'a Yaml, key: &str) -> Result<&'a Yaml, &'static str> {
    let Yaml::Map(fields) = value else {
        return Err("expected map");
    };
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or("missing field")
}

#[test]
fn fixed_probe_runs_only_after_admission_in_protected_job() -> Result<(), Box<dyn std::error::Error>>
{
    let job = spec().job()?;
    let text = render_yaml(&job);
    assert!(text.contains("github.event.inputs.mode == 'oidc-qualification'"));
    assert!(text.contains("github.event_name == 'workflow_dispatch'"));
    assert!(text.contains("refs/heads/main"));
    assert!(!text.contains("oidc-qualification-admit"));
    assert!(!text.contains("actions/checkout"));
    assert!(!text.contains("release-plz"));
    assert!(!text.contains("contents: write"));
    assert_eq!(field(&job, "environment")?, &Yaml::str("release"));
    let Yaml::Seq(steps) = field(&job, "steps")? else {
        return Err("expected steps".into());
    };
    assert_eq!(steps.len(), 1);
    let probe = render_yaml(&steps[0]);
    assert!(probe.contains("/usr/bin/python3 -I -S -"));
    assert!(probe.contains("SSLKEYLOGFILE"));
    assert!(probe.contains("sys.exit(qualification_main())"));
    assert!(!probe.contains("sys.exit(main())"));
    assert!(!probe.contains("velnor-actions-"));
    let Yaml::Str(command) = field(&steps[0], "run")? else {
        return Err("expected shell command".into());
    };
    let result = std::process::Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-p",
            "-e",
            "-o",
            "pipefail",
            "-c",
            command,
        ])
        .env_clear()
        .output()?;
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8(result.stderr)?.trim(),
        "OIDC qualification failed"
    );
    let fixture = r#"
import os, pathlib, ssl, sys, urllib.request
from unittest.mock import patch
sys.path.insert(0, sys.argv[1])
import oidc_inline_fixture as f
command = sys.argv[2]
marker = "VELNOR_OIDC_PROBE_PY"
assert command.count(marker) == 2
script = command.split("<<'" + marker + "'\n", 1)[1].rsplit("\n" + marker, 1)[0]
assert '${{' not in script
remote = f.Remote()
with patch.dict(os.environ, f.context(), clear=True), \
     patch.object(ssl, 'create_default_context', return_value=object()), \
     patch.object(urllib.request, 'build_opener', return_value=remote):
    try:
        exec(compile(script, '<rendered-qualification>', 'exec'), {'__name__': '__main__'})
    except SystemExit as result:
        assert result.code == 0
    else:
        raise AssertionError('rendered entrypoint did not exit')
assert [method for method, host, _ in remote.calls if host == 'crates.io'] == ['POST', 'DELETE']
"#;
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let result = std::process::Command::new("python3")
        .args(["-c", fixture])
        .arg(tests)
        .arg(command)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

#[test]
fn rejects_untrusted_bindings_and_invalid_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    let mut invalid = spec();
    invalid.default_branch = "main' || true".into();
    assert!(invalid.job().is_err());
    invalid = spec();
    invalid.environment = "release\nwrite".into();
    assert!(invalid.job().is_err());
    invalid = spec();
    invalid.required_workflow_path = ".github/workflows/../evil.yml".into();
    assert!(invalid.job().is_err());
    invalid = spec();
    invalid.trusted_reviewer_id = 0;
    assert!(invalid.job().is_err());
    let rejected = render_yaml(&spec().invalid_dispatch_job(None)?);
    assert!(rejected.contains("mode == 'oidc-qualification'"));
    assert!(rejected.contains("exit 1"));
    assert!(!rejected.contains("id-token: write"));
    Ok(())
}

fn mutations(value: &Yaml) -> Vec<Yaml> {
    let mut changed = vec![Yaml::Null];
    match value {
        Yaml::Map(fields) => {
            for index in 0..fields.len() {
                let mut removed = fields.clone();
                removed.remove(index);
                changed.push(Yaml::Map(removed));
                for child in mutations(&fields[index].1) {
                    let mut altered = fields.clone();
                    altered[index].1 = child;
                    changed.push(Yaml::Map(altered));
                }
            }
            let mut added = fields.clone();
            added.push(("unexpected".into(), Yaml::str("value")));
            changed.push(Yaml::Map(added));
        }
        Yaml::Seq(items) => {
            for index in 0..items.len() {
                let mut removed = items.clone();
                removed.remove(index);
                changed.push(Yaml::Seq(removed));
                for child in mutations(&items[index]) {
                    let mut altered = items.clone();
                    altered[index] = child;
                    changed.push(Yaml::Seq(altered));
                }
            }
            let mut added = items.clone();
            added.push(Yaml::str("unexpected"));
            changed.push(Yaml::Seq(added));
        }
        _ => changed.push(Yaml::str("unexpected")),
    }
    changed
        .into_iter()
        .filter(|candidate| candidate != value)
        .collect()
}

#[test]
fn rejects_every_structural_mutation_of_qualification_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let spec = spec();
    let plan = velnor_actions_workflow_renderer::release_spec::ReleasePlan {
        plan_id: "release-123".into(),
        repository: "owner/project".into(),
        source_sha: "a".repeat(40),
        registry: "crates-io".into(),
        packages: BTreeMap::default(),
        version: Some("0.1.1".into()),
        dispatch: true,
        qualification_dispatch:
            velnor_actions_workflow_renderer::release_spec::QualificationDispatch::Enabled,
    };
    for release_dispatch in [None, Some(&plan)] {
        let job = spec.job()?;
        let invalid = spec.invalid_dispatch_job(release_dispatch)?;
        spec.validate_jobs(&job, &invalid, release_dispatch)?;
        for altered in mutations(&job) {
            assert!(
                spec.validate_jobs(&altered, &invalid, release_dispatch)
                    .is_err()
            );
        }
        for altered in mutations(&invalid) {
            assert!(
                spec.validate_jobs(&job, &altered, release_dispatch)
                    .is_err()
            );
        }
    }
    Ok(())
}

#[test]
fn rejects_dispatch_with_mismatched_release_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let plan = velnor_actions_workflow_renderer::release_spec::ReleasePlan {
        plan_id: "release-123".into(),
        repository: "owner/project".into(),
        source_sha: "a".repeat(40),
        registry: "crates-io".into(),
        packages: BTreeMap::default(),
        version: Some("0.1.1".into()),
        dispatch: true,
        qualification_dispatch:
            velnor_actions_workflow_renderer::release_spec::QualificationDispatch::Enabled,
    };
    let preflight = velnor_actions_workflow_renderer::release_spec::preflight_gate_condition(
        "owner/project",
        "main",
        &plan,
    )
    .ok_or("preflight")?;
    let rejection = render_yaml(&spec().invalid_dispatch_job(Some(&plan))?);
    for field in [
        "github.event.inputs.mode == 'release'",
        "github.event.inputs.plan == 'release-123'",
        "github.event.inputs.source_sha == '",
        "github.event.inputs.version == '0.1.1'",
    ] {
        assert!(preflight.contains(field));
        assert!(rejection.contains(field));
    }
    Ok(())
}

#[test]
fn rejects_qualification_intent_mismatches() -> Result<(), Box<dyn std::error::Error>> {
    let spec = spec();
    let original = intent();
    spec.validate_intent(&original)?;
    let mut disabled = original.clone();
    disabled.oidc_qualification = false;
    assert!(spec.validate_intent(&disabled).is_err());
    let mut changed = original.clone();
    changed.repository = "other/project".into();
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.default_branch = "other".into();
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.environment = "other".into();
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.required_workflow_path = ".github/workflows/other.yml".into();
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.trusted_reviewer_id = 43;
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.generator_version = "0.1.79".into();
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.generator_source_sha = "c".repeat(40);
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original.clone();
    changed.generator_manifest_sha256 = "d".repeat(64);
    assert!(spec.validate_intent(&changed).is_err());
    let mut changed = original;
    changed.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    assert!(spec.validate_intent(&changed).is_err());
    Ok(())
}

#[test]
fn inline_qualification_behavior_suite_runs_under_nextest() -> Result<(), Box<dyn std::error::Error>>
{
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let driver = r"
import sys, unittest
suite = unittest.defaultTestLoader.discover(sys.argv[1], pattern='oidc_inline_fixture.py')
if suite.countTestCases() == 0:
    raise SystemExit('OIDC qualification suite is empty')
raise SystemExit(not unittest.TextTestRunner().run(suite).wasSuccessful())
";
    let result = std::process::Command::new("python3")
        .args(["-B", "-c", driver])
        .arg(tests)
        .output()?;
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
