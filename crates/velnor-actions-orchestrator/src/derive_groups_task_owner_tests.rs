//! Velnor repository-maintenance task-owner behavior.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{FileIndex, WorkflowPolicy, build_index_from_list};
use velnor_actions_mise::ArchivePlan;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile,
    TargetRecord, TaskKind, TestRunner, VelnorV1TaskOwner, WorkspaceRecord,
    tasks::cargo_payload_argv,
};

use crate::config::load_config as read_repository_config;
use crate::discover::PlannedWorkspace;

use super::{declared_union, derive_for_config};

const SUPPORT_MEMBERS: [(&str, &str); 2] = [
    (
        "crates/velnor-actions-freshness/Cargo.toml",
        "velnor-actions-freshness",
    ),
    (
        "crates/velnor-archive-guard/Cargo.toml",
        "velnor-archive-guard",
    ),
];

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = manifest
        .parent()
        .ok_or_else(|| std::io::Error::other("crate parent missing"))?;
    Ok(crates
        .parent()
        .ok_or_else(|| std::io::Error::other("workspace root missing"))?
        .to_path_buf())
}

fn index(root: &Path) -> Result<FileIndex, Box<dyn Error>> {
    let files = SUPPORT_MEMBERS
        .iter()
        .map(|(manifest, _)| (*manifest).to_owned())
        .collect::<Vec<_>>();
    Ok(build_index_from_list(root, &files, &[])?)
}

fn support_package(manifest: &str, name: &str, owner: VelnorV1TaskOwner) -> PackageRecord {
    PackageRecord {
        id: format!("{name} 0.1.0"),
        name: name.to_owned(),
        version: "0.1.0".to_owned(),
        manifest: manifest.to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![TargetRecord {
            kind: "lib".to_owned(),
            name: name.replace('-', "_"),
            test: true,
            doctest: true,
            required_features: Vec::new(),
        }],
        features: vec!["repository-only-feature".to_owned()],
        has_build_script: false,
        v1_task_owner: owner,
    }
}

fn workspace(owner: VelnorV1TaskOwner) -> PlannedWorkspace {
    let packages = SUPPORT_MEMBERS
        .iter()
        .map(|(manifest, name)| support_package(manifest, name, owner))
        .collect::<Vec<_>>();
    let members = packages.iter().map(|package| package.id.clone()).collect();
    PlannedWorkspace {
        record: WorkspaceRecord {
            workspace_root: String::new(),
            members,
            packages,
            edges: Vec::new(),
            skipped_edges: Vec::new(),
            has_external_dependencies: false,
        },
        profile: RustExecutionProfile {
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            evidence: Vec::new(),
            driver_source: ProfileSource::Declared,
            runner_source: ProfileSource::Declared,
            nextest_profile: NextestProfile::Default,
            nextest_config: None,
            run_ignored: None,
        },
        recommendations: Vec::new(),
        findings: Vec::new(),
    }
}

fn derive(
    config: &velnor_actions_contract::VelnorConfig,
    index: &FileIndex,
    workspace: &PlannedWorkspace,
) -> Result<Vec<velnor_actions_rust::TaskGroup>, Box<dyn Error>> {
    let rust = config
        .stacks
        .rust
        .as_ref()
        .ok_or_else(|| std::io::Error::other("Rust stack config missing"))?;
    let rust_config = rust
        .configurations
        .first()
        .ok_or_else(|| std::io::Error::other("Rust configuration missing"))?;
    let (groups, _) = derive_for_config(
        config,
        index,
        workspace,
        rust_config,
        false,
        &mut ArchivePlan::new(),
        &BTreeSet::new(),
    )?;
    Ok(groups)
}

#[test]
fn repository_maintenance_owner_is_scoped_to_the_canonical_repository_policy()
-> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let index = index(&root)?;
    let mut repository_config = read_repository_config(&root)?;
    repository_config.workflow.policy = WorkflowPolicy::VelnorRepositoryV1;
    let support = workspace(VelnorV1TaskOwner::RepositoryMaintenance);

    assert_eq!(
        derive(&repository_config, &index, &support)?,
        [] as [velnor_actions_rust::TaskGroup; 0]
    );
    assert!(
        declared_union(
            std::slice::from_ref(&support),
            &index,
            WorkflowPolicy::VelnorRepositoryV1
        )
        .is_empty()
    );

    let mut consumer_config = repository_config.clone();
    consumer_config.workflow.policy = WorkflowPolicy::ConsumerV1;
    assert_ne!(
        derive(&consumer_config, &index, &support)?,
        [] as [velnor_actions_rust::TaskGroup; 0]
    );
    assert!(
        declared_union(
            std::slice::from_ref(&support),
            &index,
            WorkflowPolicy::ConsumerV1
        )
        .contains("repository-only-feature")
    );

    let same_name_without_owner = workspace(VelnorV1TaskOwner::Project);
    assert_ne!(
        derive(&repository_config, &index, &same_name_without_owner)?,
        [] as [velnor_actions_rust::TaskGroup; 0]
    );
    Ok(())
}

/// Groups for the first configuration of `config` with explicit formatting.
fn derive_with_fmt(
    config: &velnor_actions_contract::VelnorConfig,
    index: &FileIndex,
    workspace: &PlannedWorkspace,
) -> Result<Vec<velnor_actions_rust::TaskGroup>, Box<dyn Error>> {
    let rust = config
        .stacks
        .rust
        .as_ref()
        .ok_or_else(|| std::io::Error::other("Rust stack config missing"))?;
    let union = declared_union(
        std::slice::from_ref(workspace),
        index,
        config.workflow.policy,
    );
    let (groups, _) = derive_for_config(
        config,
        index,
        workspace,
        &rust.configurations[0],
        true,
        &mut ArchivePlan::new(),
        &union,
    )?;
    Ok(groups)
}

fn load_test_config(
    contents: &str,
) -> Result<velnor_actions_contract::VelnorConfig, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_dir = root.path().join(".velnor");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(config_dir.join("config.toml"), contents)?;
    Ok(read_repository_config(root.path())?)
}

#[test]
fn loaded_clippy_all_features_config_changes_only_the_clippy_task() -> Result<(), Box<dyn Error>> {
    let enabled = load_test_config("schema = 1\n[stacks.rust]\nclippy_all_features = true\n")?;
    let default = load_test_config("schema = 1\n[stacks.rust]\n")?;
    let index = index(&repository_root()?)?;
    let workspace = workspace(VelnorV1TaskOwner::Project);
    let enabled_groups = derive_with_fmt(&enabled, &index, &workspace)?;
    let default_groups = derive_with_fmt(&default, &index, &workspace)?;
    let ids = |groups: &[velnor_actions_rust::TaskGroup]| {
        groups
            .iter()
            .map(|group| group.task_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&enabled_groups), ids(&default_groups));
    assert!(
        enabled_groups
            .iter()
            .any(|group| group.kind == TaskKind::Fmt)
    );
    assert_eq!(
        enabled_groups
            .iter()
            .filter(|group| group.kind == TaskKind::Clippy)
            .count(),
        workspace.record.packages.len()
    );
    for (group, default_group) in enabled_groups.iter().zip(&default_groups) {
        let argv = cargo_payload_argv(group)?;
        if group.kind == TaskKind::Clippy {
            assert!(argv.iter().any(|arg| arg == "--all-features"), "{argv:?}");
        } else {
            assert_eq!(
                group,
                default_group,
                "{} must not change",
                group.kind.as_str()
            );
        }
        assert!(
            !cargo_payload_argv(default_group)?
                .iter()
                .any(|arg| arg == "--all-features")
        );
    }
    Ok(())
}

#[test]
fn all_features_configuration_selects_each_package_feature_set() -> Result<(), Box<dyn Error>> {
    let config = load_test_config(
        "schema = 1\n[[stacks.rust.configurations]]\nname = \"all\"\nall_features = true\ntarget = \"host\"\n",
    )?;
    let index = index(&repository_root()?)?;
    let workspace = workspace(VelnorV1TaskOwner::Project);
    let groups = derive_with_fmt(&config, &index, &workspace)?;
    assert_ne!(groups, [] as [velnor_actions_rust::TaskGroup; 0]);
    for group in &groups {
        let argv = cargo_payload_argv(group)?;
        assert!(group.all_features, "{}", group.task_id);
        assert!(group.task_id.ends_with("/all"), "{}", group.task_id);
        assert_eq!(group.features, ["repository-only-feature"]);
        assert_eq!(
            argv.iter().any(|arg| arg == "--all-features"),
            group.kind != TaskKind::Fmt,
            "{argv:?}"
        );
    }
    Ok(())
}
