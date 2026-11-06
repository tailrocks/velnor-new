// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::CrateReleaseIntent;

#[test]
fn intent_rejects_unapproved_or_ambiguous_release_inputs() -> Result<(), Box<dyn std::error::Error>>
{
    let value = serde_json::json!({"schema": 1, "repository": "example/demo-crate", "default_branch": "main", "environment": "release", "required_workflow_path": ".github/workflows/ci.yml", "trusted_reviewer_id": 139_017, "generator_version":"0.1.72","generator_source_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","generator_manifest_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","rust_version": "1.97.1", "manifest_path": "Cargo.toml", "pr_write_paths": ["Cargo.toml"], "registry": "crates-io", "packages": ["demo-crate"], "publish_features": {"demo-crate":[]}, "tag_template": "{{ package }}-v{{ version }}", "recovery": {"package": "demo-crate", "version": "0.1.1", "source_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}});
    let intent: CrateReleaseIntent = serde_json::from_value(value.clone())?;
    intent.validate()?;
    let mut hidden = intent.clone();
    hidden.pr_write_paths = vec![".private/Cargo.toml".into(), "Cargo.toml".into()];
    hidden.validate()?;
    hidden.pr_write_paths = vec!["Cargo.toml".into(), "crates/é space/Cargo.toml".into()];
    hidden.validate()?;
    for (key, invalid) in [
        ("generator_version", serde_json::json!("latest")),
        ("generator_source_sha", serde_json::json!("invalid")),
        ("generator_manifest_sha256", serde_json::json!("invalid")),
        ("release_plz_version", serde_json::json!("0.4")),
        ("runner", serde_json::json!("${{ inputs.runner }}")),
        ("authentication", serde_json::json!("unknown")),
        ("repository", serde_json::json!("example/demo-crate/other")),
        ("default_branch", serde_json::json!("../main")),
        ("environment", serde_json::json!("release\nother")),
        ("manifest_path", serde_json::json!("../Cargo.toml")),
        ("registry", serde_json::json!("another")),
        ("pr_write_paths", serde_json::json!([])),
        ("pr_write_paths", serde_json::json!(["Cargo.lock"])),
        (
            "pr_write_paths",
            serde_json::json!(["Cargo.toml", "Cargo.toml"]),
        ),
        (
            "pr_write_paths",
            serde_json::json!(["Cargo.toml", "../CHANGELOG.md"]),
        ),
        (
            "pr_write_paths",
            serde_json::json!(["Cargo.toml", "src/lib.rs"]),
        ),
        (
            "pr_write_paths",
            serde_json::json!([".git/Cargo.toml", "Cargo.toml"]),
        ),
        (
            "pr_write_paths",
            serde_json::json!(["Cargo.toml", "CHANGELOG.md"]),
        ),
        ("publish_features", serde_json::json!({})),
        ("publish_features", serde_json::json!({"other": []})),
        (
            "publish_features",
            serde_json::json!({"demo-crate": ["pty", "pty"]}),
        ),
        ("packages", serde_json::json!(["other"])),
        ("tag_template", serde_json::json!("v{{ version }}")),
    ] {
        let mut candidate = value.clone();
        candidate[key] = invalid;
        let result = serde_json::from_value::<CrateReleaseIntent>(candidate);
        assert!(
            result.is_err() || result?.validate().is_err(),
            "invalid {key} was accepted"
        );
    }
    for missing in [
        "generator_version",
        "generator_source_sha",
        "generator_manifest_sha256",
        "release_plz_version",
        "runner",
        "authentication",
        "publish_features",
        "pr_write_paths",
    ] {
        let mut candidate = value.clone();
        candidate.as_object_mut().ok_or("object")?.remove(missing);
        assert!(serde_json::from_value::<CrateReleaseIntent>(candidate).is_err());
    }
    let mut extra = value;
    extra["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CrateReleaseIntent>(extra).is_err());
    Ok(())
}

#[test]
fn recovery_accepts_cargo_semver_and_rejects_invalid_versions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut value = serde_json::json!({"schema":1,"repository":"example/demo-crate","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"generator_version":"0.1.72","generator_source_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","generator_manifest_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","rust_version":"1.97.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],"registry":"crates-io","packages":["demo-crate"],"publish_features":{"demo-crate":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":{"package":"demo-crate","version":"1.2.3-alpha.1+build.5","source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}});
    serde_json::from_value::<CrateReleaseIntent>(value.clone())?.validate()?;
    let maximum = format!("1.2.3-{}", "a".repeat(144));
    value["recovery"]["version"] = serde_json::json!(maximum);
    serde_json::from_value::<CrateReleaseIntent>(value.clone())?.validate()?;
    value["recovery"]["version"] = serde_json::json!(format!("{maximum}a"));
    assert!(
        serde_json::from_value::<CrateReleaseIntent>(value.clone())?
            .validate()
            .is_err()
    );
    for invalid in [
        "01.2.3", "1.02.3", "1.2.3-01", "1.2.3-", "1.2.3+", " 1.2.3", "1.2",
    ] {
        value["recovery"]["version"] = serde_json::json!(invalid);
        assert!(
            serde_json::from_value::<CrateReleaseIntent>(value.clone())?
                .validate()
                .is_err(),
            "accepted {invalid}"
        );
    }
    Ok(())
}

#[test]
fn configured_package_features_are_safe_and_owned() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"publish_features":{"demo":["dep/feature","pty"]}});
    let config: RustReleaseConfig = serde_json::from_value(input.clone())?;
    config.validate("test")?;
    for (package, features) in [
        ("unknown", vec!["pty"]),
        ("demo", vec!["pty", "pty"]),
        ("demo", vec!["bad feature"]),
        ("demo", vec!["z", "a"]),
    ] {
        let mut wrong = input.clone();
        wrong["publish_features"] = serde_json::json!({package:features});
        let parsed: RustReleaseConfig = serde_json::from_value(wrong)?;
        assert!(parsed.validate("test").is_err());
    }
    Ok(())
}

#[test]
fn reviewed_new_crate_scope_requires_exact_package_version_and_bootstrap_authentication()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let mut input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"authentication":"bootstrap-token","new_crates":{"source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","packages":{"demo":"0.1.0"}}});
    serde_json::from_value::<RustReleaseConfig>(input.clone())?.validate("test")?;
    for invalid in [
        serde_json::json!({"other":"0.1.0"}),
        serde_json::json!({"demo":"latest"}),
        serde_json::json!({"demo":""}),
    ] {
        input["new_crates"]["packages"] = invalid;
        assert!(
            serde_json::from_value::<RustReleaseConfig>(input.clone())?
                .validate("test")
                .is_err()
        );
    }
    input["new_crates"]["packages"] = serde_json::json!({"demo":"0.1.0"});
    input["authentication"] = serde_json::json!("trusted-publishing");
    assert!(
        serde_json::from_value::<RustReleaseConfig>(input.clone())?
            .validate("test")
            .is_err()
    );
    input
        .as_object_mut()
        .ok_or("configuration")?
        .remove("new_crates");
    serde_json::from_value::<RustReleaseConfig>(input)?.validate("test")?;
    Ok(())
}

#[test]
fn bootstrap_workspace_creation_has_explicit_source_and_each_version()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["alpha","beta"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"authentication":"bootstrap-token","new_crates":{"source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","packages":{"alpha":"0.1.0","beta":"0.2.0"}}});
    let config: RustReleaseConfig = serde_json::from_value(input)?;
    config.validate("test")?;
    Ok(())
}

#[test]
fn bootstrap_workspace_creation_respects_release_package_limit()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::{MAX_CRATE_RELEASE_PACKAGES, RustReleaseConfig};
    let packages: serde_json::Map<String, serde_json::Value> = (0..MAX_CRATE_RELEASE_PACKAGES)
        .map(|index| (format!("crate-{index}"), "0.1.0".into()))
        .collect();
    let mut input = serde_json::json!({"enabled":true,"registry":"crates-io","publishable_workspace":true,"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"authentication":"bootstrap-token","new_crates":{"source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","packages":packages}});
    serde_json::from_value::<RustReleaseConfig>(input.clone())?.validate("test")?;
    input["new_crates"]["packages"]["crate-extra"] = "0.1.0".into();
    assert!(
        serde_json::from_value::<RustReleaseConfig>(input)?
            .validate("test")
            .is_err()
    );
    Ok(())
}

#[test]
fn release_environment_names_are_valid_github_routes() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let mut input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
    for name in [
        "release".to_owned(),
        "prod/release".to_owned(),
        "a".repeat(255),
    ] {
        input["environment"] = name.into();
        serde_json::from_value::<RustReleaseConfig>(input.clone())?.validate("test")?;
    }
    for name in [".".to_owned(), "prod/./release".to_owned(), "a".repeat(256)] {
        input["environment"] = name.into();
        assert!(
            serde_json::from_value::<RustReleaseConfig>(input.clone())?
                .validate("test")
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn predecessor_accepts_only_first_run_attempt() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let mut input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"journal_predecessor":{"run_id":12,"run_attempt":1,"workflow_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}});
    assert!(
        serde_json::from_value::<RustReleaseConfig>(input.clone())?
            .validate("test")
            .is_err()
    );
    input["recovery"] = serde_json::json!({"package":"demo","version":"0.1.1","source_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"});
    serde_json::from_value::<RustReleaseConfig>(input.clone())?.validate("test")?;
    for attempt in [0, 2] {
        input["journal_predecessor"]["run_attempt"] = attempt.into();
        assert!(
            serde_json::from_value::<RustReleaseConfig>(input.clone())?
                .validate("test")
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn release_config_rejects_unusable_admission_and_tag_settings()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let mut input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
    serde_json::from_value::<RustReleaseConfig>(input.clone())?.validate("test")?;
    for (field, value) in [
        ("trusted_reviewer_id", serde_json::json!(0)),
        ("required_workflow_path", serde_json::json!("../../ci.yml")),
        (
            "tag_name",
            serde_json::json!("release/{{ package }}-{{ version }}"),
        ),
    ] {
        let mut invalid = input.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<RustReleaseConfig>(invalid)?
                .validate("test")
                .is_err()
        );
    }
    input["enabled"] = false.into();
    input["trusted_reviewer_id"] = 0.into();
    serde_json::from_value::<RustReleaseConfig>(input)?.validate("test")?;
    Ok(())
}

#[test]
fn bootstrap_creation_cannot_include_undeclared_packages() -> Result<(), Box<dyn std::error::Error>>
{
    use velnor_actions_contract::config::RustReleaseConfig;
    let input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["alpha","beta"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"authentication":"bootstrap-token","new_crates":{"source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","packages":{"alpha":"0.1.0"}}});
    assert!(
        serde_json::from_value::<RustReleaseConfig>(input)?
            .validate("test")
            .is_err()
    );
    Ok(())
}

#[test]
fn release_rejects_more_packages_than_intent_supports() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let packages: Vec<String> = (0..65).map(|n| format!("package-{n:02}")).collect();
    let input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":packages,"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
    assert!(
        serde_json::from_value::<RustReleaseConfig>(input)?
            .validate("test")
            .is_err()
    );
    Ok(())
}
