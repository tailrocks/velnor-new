//! SDK recipe authority cannot be replaced by neutral structural records.
use super::*;
#[test]
fn homebrew_recipe_is_anonymous_and_binds_catalog_source_and_dynamic_checkout()
-> Result<(), Box<dyn std::error::Error>> {
    let semantic = NativeValidationDescriptor::HomebrewPreparation {
        repository: "orbit/homebrew-nebula".to_owned(),
        has_casks: false,
    };
    let catalog = ToolCatalog::pinned();
    let recipe = recipe_for_descriptor(&semantic, &catalog)?;
    assert!(recipe.is_homebrew_foundation());
    assert!(recipe.installed_selectors().is_empty());
    assert_eq!(
        recipe
            .environment()
            .get("VELNOR_NATIVE_VALIDATION_SOURCE_SHA")
            .map(String::as_str),
        Some("${{ github.sha }}")
    );
    assert!(
        recipe.prefix().iter().any(|value| value
            == "VELNOR_NATIVE_VALIDATION_SOURCE_SHA=$VELNOR_NATIVE_VALIDATION_SOURCE_SHA")
    );
    assert!(
        !recipe
            .environment()
            .keys()
            .any(|key| crate::command::is_denied_credential_key(key))
    );
    validate_recipe(&semantic, &catalog, &recipe)?;
    let changed = CompiledNativeExecRecipe::compiled_homebrew_foundation(
        vec![
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "/usr/bin/env".to_owned(),
            "--".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    assert!(validate_recipe(&semantic, &catalog, &changed).is_err());
    Ok(())
}
#[test]
fn invalid_descriptor_and_foreign_catalog_fail() -> Result<(), Box<dyn std::error::Error>> {
    let semantic = NativeValidationDescriptor::HomebrewPreparation {
        repository: "Orbit/homebrew-nebula".to_owned(),
        has_casks: false,
    };
    assert!(recipe_for_descriptor(&semantic, &ToolCatalog::pinned()).is_err());
    let catalog = ToolCatalog::pinned();
    let foreign = ToolCatalog::new(
        "9.9.9",
        catalog.version(PinnedTool::MrBoxington),
        catalog.version(PinnedTool::Gh),
        catalog.version(PinnedTool::Actionlint),
        catalog.version(PinnedTool::Shellcheck),
        catalog.version(PinnedTool::Zizmor),
        catalog.version(PinnedTool::Nextest),
        catalog.version(PinnedTool::Opentofu),
    )?;
    let semantic = NativeValidationDescriptor::HomebrewPreparation {
        repository: "orbit/homebrew-nebula".to_owned(),
        has_casks: false,
    };
    assert!(recipe_for_descriptor(&semantic, &foreign).is_err());
    Ok(())
}

use velnor_actions_contract::config::{
    PackageUpdateArchive, PackageUpdateArtifact, PackageUpdateFixture, PackageUpdateOutput,
};
fn package() -> NativeValidationDescriptor {
    NativeValidationDescriptor::PackageUpdateFixture {
        profile: PackageUpdateFixture {
            updater: "scripts/package-update.sh".to_owned(),
            repository: "orbit-tools/nebula".to_owned(),
            formula: "Formula/nebula.rb".to_owned(),
            preview_formula: "Formula/nebula-preview.rb".to_owned(),
            cask: None,
            binary: "nebula".to_owned(),
            artifacts: vec![PackageUpdateArtifact {
                prefix: "nebula".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
                archive: PackageUpdateArchive::TarGz,
                output: PackageUpdateOutput::Formula,
                preview: true,
                executable: true,
            }],
            supporting_manifests: Vec::new(),
        },
    }
}

#[test]
fn package_recipe_has_exact_installed_ruby_jq_and_no_implicit_install()
-> Result<(), Box<dyn std::error::Error>> {
    let catalog = ToolCatalog::pinned();
    let semantic = package();
    let recipe = recipe_for_descriptor(&semantic, &catalog)?;
    assert_eq!(
        recipe.installed_selectors(),
        catalog.tool_specs(&[PinnedTool::Ruby, PinnedTool::Jq])?
    );
    assert!(!recipe.is_homebrew_foundation());
    for key in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
        assert_eq!(
            recipe.environment().get(key).map(String::as_str),
            Some("false")
        );
    }
    assert!(
        recipe
            .prefix()
            .iter()
            .any(|value| value == "$RUNNER_TEMP/velnor/mise/bin/mise")
    );
    validate_recipe(&semantic, &catalog, &recipe)?;
    Ok(())
}
