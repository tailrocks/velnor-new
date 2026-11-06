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
    let packages: Vec<String> = (0..65).map(|n| format!("package-{n:02}")).collect();
    let input = serde_json::json!({"enabled":true,"registry":"crates-io","packages":packages,"required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1});
    assert!(
        serde_json::from_value::<RustReleaseConfig>(input)?
            .validate("test")
            .is_err()
    );
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
