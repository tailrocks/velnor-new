// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
use velnor_actions_workflow_renderer::release_config::{
    ReleasePlzConfig, ReleasePlzPackage, render_single_package_upload_config,
};

#[test]
fn upload_config_selects_one_package_and_preserves_policy() -> Result<(), Box<dyn std::error::Error>>
{
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: false,
        packages: vec![
            ReleasePlzPackage {
                version_group: Some("shared".into()),
                name: "velnor_probe_a".into(),
                publish_features: vec!["demo".into()],
            },
            ReleasePlzPackage {
                version_group: Some("shared".into()),
                name: "velnor_probe_b".into(),
                publish_features: vec![],
            },
        ],
    };
    {
        let text = render_single_package_upload_config(&config, "velnor_probe_a", "0.1.72")?;
        let value: toml::Value = toml::from_str(&text)?;
        let workspace = &value["workspace"];
        assert_eq!(workspace["release"].as_bool(), Some(false));
        assert_eq!(workspace["release_always"].as_bool(), Some(true));
        assert_eq!(workspace["pr_branch_prefix"].as_str(), Some(":"));
        assert_eq!(workspace["git_tag_enable"].as_bool(), Some(false));
        assert_eq!(workspace["git_release_enable"].as_bool(), Some(false));
        assert_eq!(workspace["publish_no_verify"].as_bool(), Some(false));
        let packages = value["package"].as_array().ok_or("missing packages")?;
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0]["name"].as_str(), Some("velnor_probe_a"));
        assert_eq!(packages[0]["release"].as_bool(), Some(true));
        assert!(packages[0].get("version_group").is_none());
        assert_eq!(packages[0]["publish_features"][0].as_str(), Some("demo"));
    }
    assert!(render_single_package_upload_config(&config, "missing", "0.1.72").is_err());
    assert!(render_single_package_upload_config(&config, "a\"\nrelease=true", "0.1.72").is_err());
    let mut duplicate = config.clone();
    duplicate.packages.push(config.packages[0].clone());
    assert!(render_single_package_upload_config(&duplicate, "velnor_probe_a", "0.1.72").is_err());
    Ok(())
}
