//! Native source reconstruction remains independent of executable distribution owners.
use super::*;
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

fn brew(has_casks: bool) -> NativeValidationDescriptor {
    NativeValidationDescriptor::HomebrewPreparation {
        repository: "orbit-tools/homebrew-nebula".to_owned(),
        has_casks,
    }
}

fn recipe(homebrew: bool) -> Result<CompiledNativeExecRecipe, ContractError> {
    let mut env = BTreeMap::from([
        (
            "HOME".to_owned(),
            "${{ runner.temp }}/native-home".to_owned(),
        ),
        (
            "VELNOR_NATIVE_VALIDATION_SOURCE_SHA".to_owned(),
            "${{ github.sha }}".to_owned(),
        ),
    ]);
    let identity = if homebrew {
        reviewed()?.recipe()
    } else {
        "package-update-fixture-v1".to_owned()
    };
    env.insert(
        "VELNOR_NATIVE_VALIDATION_RECIPE".to_owned(),
        velnor_actions_contract::digest_b3(identity.as_bytes()),
    );
    if homebrew {
        return CompiledNativeExecRecipe::compiled_homebrew_foundation(
            vec![
                "/usr/bin/env".to_owned(),
                "-i".to_owned(),
                "/usr/bin/env".to_owned(),
                "--".to_owned(),
            ],
            env,
        );
    }
    let selectors = vec!["ruby@4.0.7".to_owned(), "jq@1.8.1".to_owned()];
    let mut prefix = vec![
        "/usr/bin/env".to_owned(),
        "-i".to_owned(),
        "/owned/mise".to_owned(),
        "exec".to_owned(),
    ];
    prefix.extend(selectors.clone());
    prefix.push("--".to_owned());
    CompiledNativeExecRecipe::compiled(prefix, env, selectors)
}
fn reviewed() -> Result<audit::BrewSourceIdentity, ContractError> {
    audit::BrewSourceIdentity::reviewed(
        "7.0.7",
        "8e858db5584704dcd469b8e826228c0d5a5a94f6",
        "4.0.7",
        &"a".repeat(64),
        &"b".repeat(64),
    )
}
#[test]
fn complete_fixed_source_and_semantics_rebuild_exactly() -> Result<(), Box<dyn std::error::Error>> {
    let reviewed = reviewed()?;
    for semantic in [package(), brew(false), brew(true)] {
        let homebrew = matches!(
            semantic,
            NativeValidationDescriptor::HomebrewPreparation { .. }
        );
        let recipe = recipe(homebrew)?;
        let record = record_for_descriptor(&semantic, Some(&reviewed), recipe.clone(), "0.1.0")?;
        assert_eq!(
            record.invocation().native_validation_descriptor(),
            Some(&semantic)
        );
        assert!(
            record
                .source()
                .contains("native_validation_source_mismatch")
        );
        assert_eq!(
            compiled_source_sha256(record.source().as_bytes()),
            record.invocation().descriptor().source_sha256()
        );
        assert_eq!(
            record_for_invocation(
                record.invocation(),
                record.environment(),
                Some(&reviewed),
                recipe,
                "0.1.0"
            )?,
            record
        );
    }
    Ok(())
}
#[test]
fn wire_mutations_and_missing_reviewed_source_fail() -> Result<(), Box<dyn std::error::Error>> {
    let reviewed = reviewed()?;
    let semantic = brew(false);
    assert!(record_for_descriptor(&semantic, None, recipe(true)?, "0.1.0").is_err());
    let foreign = audit::BrewSourceIdentity::reviewed(
        "7.0.8",
        "8e858db5584704dcd469b8e826228c0d5a5a94f6",
        "4.0.7",
        &"a".repeat(64),
        &"b".repeat(64),
    )?;
    assert!(record_for_descriptor(&semantic, Some(&foreign), recipe(true)?, "0.1.0").is_err());
    assert!(record_for_descriptor(&semantic, Some(&reviewed), recipe(false)?, "0.1.0").is_err());
    assert!(record_for_descriptor(&package(), None, recipe(true)?, "0.1.0").is_err());
    let record = record_for_descriptor(&package(), None, recipe(false)?, "0.1.0")?;
    let wire = serde_json::to_value(record.invocation())?;
    for (pointer, replacement) in [
        ("/args/0", serde_json::json!("arbitrary-program")),
        ("/helper/source_sha256", serde_json::json!("a".repeat(64))),
        ("/installed_selectors", serde_json::json!(["ruby@9.0.0"])),
        (
            "/execution_prefix",
            serde_json::json!(["/usr/bin/env", "--"]),
        ),
    ] {
        let mut changed = wire.clone();
        *changed.pointer_mut(pointer).ok_or("mutation field")? = replacement;
        let invocation: HelperInvocation = serde_json::from_value(changed)?;
        assert!(
            record_for_invocation(
                &invocation,
                record.environment(),
                None,
                recipe(false)?,
                "0.1.0"
            )
            .is_err()
        );
    }
    let mut env = record.environment().clone();
    env.insert("GH_TOKEN".to_owned(), "forged".to_owned());
    assert!(
        record_for_invocation(record.invocation(), &env, None, recipe(false)?, "0.1.0").is_err()
    );
    assert!(
        record_for_invocation(
            record.invocation(),
            record.environment(),
            None,
            recipe(false)?,
            "0.2.0"
        )
        .is_err()
    );
    Ok(())
}
