// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
    for registry in ["crates-io", "git-tag"] {
        for (count, accepted) in [(64, true), (65, false)] {
            let packages: Vec<String> = (0..count).map(|n| format!("package-{n:02}")).collect();
            let input = serde_json::json!({"enabled":true,"registry":registry,"packages":packages,"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
            assert_eq!(
                serde_json::from_value::<RustReleaseConfig>(input)?
                    .validate("test")
                    .is_ok(),
                accepted,
                "registry={registry}, count={count}"
            );
        }
    }
    Ok(())
}

#[test]
fn runner_selection_rejects_old_ubuntu_images() {
    use velnor_actions_contract::config::{RUNNER_LABEL_CATALOG, RunsOn, ScaleSetSelector};
    for runner in ["ubuntu-26.04", "ubuntu-26.04-arm"] {
        assert!(RUNNER_LABEL_CATALOG.contains(&runner));
        assert!(RunsOn::parse(runner).is_ok());
    }
    for runner in [
        "ubuntu-22.04",
        "ubuntu-24.04",
        "ubuntu-24.04-arm",
        "ubuntu-latest",
        "ubuntu-26.04-fake",
    ] {
        assert!(!RUNNER_LABEL_CATALOG.contains(&runner));
        assert!(RunsOn::parse(runner).is_err());
        assert!(ScaleSetSelector::try_new(runner, &["velnor".into(), runner.into()]).is_err());
    }
    assert!(RunsOn::parse("macos-26").is_ok());
    assert!(RunsOn::parse("scale-set:velnor+ubuntu-26.04-scale-set").is_ok());
}

#[test]
fn crate_tag_templates_preserve_only_placeholder_whitespace()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let base = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
    for template in ["{{package}}-v{{version}}", "{{ package }}-v{{ version }}"] {
        let mut value = base.clone();
        value["tag_name"] = template.into();
        serde_json::from_value::<RustReleaseConfig>(value)?.validate("test")?;
        let concrete = template
            .replace("{{package}}", "demo")
            .replace("{{version}}", "1.2.3")
            .replace("{{ package }}", "demo")
            .replace("{{ version }}", "1.2.3");
        assert!(
            std::process::Command::new("git")
                .args(["check-ref-format", &format!("refs/tags/{concrete}")])
                .status()?
                .success()
        );
    }
    for template in [
        " {{package}}-v{{version}}",
        "{{package}} -v{{version}}",
        "{{package}}-v{{version}} ",
        "{{ pack age }}-v{{version}}",
        "{{package}}-v{{version}}{{version}}",
        "{{package}}\t-v{{version}}",
        "{{package}}-v{{version}}\n",
    ] {
        let mut value = base.clone();
        value["tag_name"] = template.into();
        assert!(
            serde_json::from_value::<RustReleaseConfig>(value)?
                .validate("test")
                .is_err(),
            "accepted {template:?}"
        );
    }
    assert_git_tag_ref_templates()?;
    Ok(())
}

#[test]
fn release_package_names_obey_registry_length_limit() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    for (name, valid) in [("a".repeat(64), true), ("a".repeat(65), false)] {
        let value = serde_json::json!({"enabled":true,"registry":"crates-io","packages":[name],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
        assert_eq!(
            serde_json::from_value::<RustReleaseConfig>(value)?
                .validate("test")
                .is_ok(),
            valid,
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn version_group_keys_are_not_limited_as_crate_names() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let group = "g".repeat(65);
    let mut value = serde_json::json!({"enabled":true,"registry":"crates-io","packages":["demo"],"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"version_groups":{group.clone():["demo"]}});
    serde_json::from_value::<RustReleaseConfig>(value.clone())?.validate("test")?;
    value["publishable_workspace"] = true.into();
    value["packages"] = serde_json::json!([]);
    serde_json::from_value::<RustReleaseConfig>(value.clone())?.validate("test")?;
    value["version_groups"][&group] = serde_json::json!(["a".repeat(65)]);
    assert!(
        serde_json::from_value::<RustReleaseConfig>(value)?
            .validate("test")
            .is_err()
    );
    Ok(())
}

fn assert_git_tag_ref_templates() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;
    let base = serde_json::json!({"enabled":true,"registry":"git-tag","packages":["demo"]});
    for (template, accepted) in [
        ("{{ package }}-v{{ version }}", true),
        ("release/{{package}}-v{{version}}", true),
        (" {{package}}-v{{version}}", false),
        ("{{package}} -v{{version}}", false),
        ("{{package}}-v{{version}} ", false),
        ("{{package}}-v{{version}}.", false),
        ("{{package}}-v{{version}}.lock", false),
        ("release//{{package}}-v{{version}}", false),
        ("release/.hidden/{{package}}-v{{version}}", false),
    ] {
        let mut input = base.clone();
        input["tag_name"] = template.into();
        assert_eq!(
            serde_json::from_value::<RustReleaseConfig>(input)?
                .validate("test")
                .is_ok(),
            accepted,
            "{template}"
        );
        for version in ["1.2.3", "1.2.3-rc.1", "1.2.3+build.7"] {
            let concrete = template
                .replace("{{package}}", "demo")
                .replace("{{ package }}", "demo")
                .replace("{{version}}", version)
                .replace("{{ version }}", version);
            assert_eq!(
                std::process::Command::new("git")
                    .args(["check-ref-format", &format!("refs/tags/{concrete}")])
                    .status()?
                    .success(),
                accepted,
                "{concrete}"
            );
        }
    }
    let mut unknown = base;
    unknown["tag_name"] = "{{package}}-v{{version}}-{{unknown}}".into();
    assert!(
        serde_json::from_value::<RustReleaseConfig>(unknown)?
            .validate("test")
            .is_err()
    );
    Ok(())
}
#[test]
fn rust_release_oidc_qualification_preserves_boolean_wire_format()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::RustReleaseConfig;

    let absent: RustReleaseConfig = serde_json::from_str("{}")?;
    let disabled: RustReleaseConfig = serde_json::from_str(r#"{"oidc_qualification":false}"#)?;
    let enabled: RustReleaseConfig = serde_json::from_str(r#"{"oidc_qualification":true}"#)?;
    assert_eq!(absent, disabled);
    assert_eq!(
        serde_json::to_value(&absent)?,
        serde_json::to_value(&disabled)?
    );
    assert!(
        serde_json::to_value(&disabled)?
            .get("oidc_qualification")
            .is_none()
    );
    assert_eq!(serde_json::to_value(&enabled)?["oidc_qualification"], true);
    assert!(
        serde_json::from_str::<RustReleaseConfig>(r#"{"oidc_qualification":"enabled"}"#).is_err()
    );

    let absent_toml: RustReleaseConfig = toml::from_str("")?;
    let disabled_toml: RustReleaseConfig = toml::from_str("oidc_qualification = false")?;
    let enabled_toml: RustReleaseConfig = toml::from_str("oidc_qualification = true")?;
    assert_eq!(absent_toml, absent);
    assert_eq!(disabled_toml, disabled);
    assert_eq!(enabled_toml, enabled);
    assert_eq!(toml::to_string(&absent)?, toml::to_string(&disabled)?);
    assert!(!toml::to_string(&disabled)?.contains("oidc_qualification"));
    assert!(toml::to_string(&enabled)?.contains("oidc_qualification = true"));
    assert!(toml::from_str::<RustReleaseConfig>("oidc_qualification = 'enabled'").is_err());
    Ok(())
}
