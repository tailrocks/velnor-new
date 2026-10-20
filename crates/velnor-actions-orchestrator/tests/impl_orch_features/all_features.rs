//! Configuration-wide Cargo all-features mode, end to end.

use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::workflow::TaskExecutionManifestV1;
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_rust::TaskKind;

use super::{feature_repo, features_of, task_manifest, task_record};
use crate::impl_common::TestResult;

/// Config with one `all` configuration selecting every package feature.
const ALL_FEATURES_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[stacks.rust.configurations]]\nname = \"all\"\nall_features = true\ntarget = \"host\"\n";

#[test]
fn all_features_uses_each_packages_complete_feature_set() -> TestResult {
    let repo = feature_repo(ALL_FEATURES_CONFIG)?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.discovery.feature_fallbacks.len(), 0);
    assert_eq!(
        features_of(&prep, "app", TaskKind::Clippy),
        vec![vec![
            "default".to_owned(),
            "process".to_owned(),
            "pty".to_owned()
        ]]
    );
    assert_eq!(
        features_of(&prep, "fuzz", TaskKind::Clippy),
        vec![vec!["extra".to_owned()]]
    );
    let tree = render_staged_tree(&prep)?;
    let manifest = task_manifest(&tree)?;
    assert_all_features_commands(&prep, &manifest)?;

    let extended_repo = feature_repo(ALL_FEATURES_CONFIG)?;
    add_app_feature(&extended_repo)?;
    let extended = prepare(extended_repo.path())?;
    assert_eq!(
        features_of(&extended, "app", TaskKind::Clippy),
        vec![vec![
            "default".to_owned(),
            "extra".to_owned(),
            "process".to_owned(),
            "pty".to_owned()
        ]]
    );
    assert_ne!(
        clippy_proposal(&prep, "app")?.identity.features,
        clippy_proposal(&extended, "app")?.identity.features
    );
    Ok(())
}

fn clippy_proposal<'a>(
    prep: &'a GenerationPreparation,
    package: &str,
) -> Result<&'a velnor_actions_contract::ProposedTask, Box<dyn Error>> {
    prep.discovery
        .proposals
        .iter()
        .find(|task| task.display_name == package && task.task_kind == "clippy")
        .ok_or_else(|| std::io::Error::other(format!("missing {package} Clippy proposal")).into())
}

fn add_app_feature(repo: &TempDir) -> Result<(), Box<dyn Error>> {
    let manifest_path = repo.path().join("crates/app/Cargo.toml");
    let original = fs::read_to_string(&manifest_path)?;
    fs::write(
        &manifest_path,
        original.replace("pty = []\n", "pty = []\nextra = []\n"),
    )?;
    Ok(())
}

fn assert_all_features_commands(
    prep: &GenerationPreparation,
    manifest: &TaskExecutionManifestV1,
) -> TestResult {
    for kind in [
        TaskKind::Clippy,
        TaskKind::Build,
        TaskKind::Nextest,
        TaskKind::Doctest,
    ] {
        let record = task_record(prep, manifest, "app", kind)?;
        assert!(
            record.argv.iter().any(|arg| arg == "--all-features"),
            "{kind:?} argv: {:?}",
            record.argv
        );
        assert!(
            !record
                .argv
                .iter()
                .any(|arg| arg == "--no-default-features" || arg == "--features"),
            "{kind:?} argv: {:?}",
            record.argv
        );
    }
    assert!(
        clippy_proposal(prep, "app")?
            .identity
            .flags
            .iter()
            .any(|flag| flag == "--all-features")
    );
    Ok(())
}
