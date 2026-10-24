//! The prebuilt `cargo_bin` steps carry the private-source block like a crate job.

use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::{
    ImageConfig, ImageDockerfile, ImagePlatform, ImagePrebuilt, Step, StepKind, StepRole,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile,
    TargetRecord, TestRunner, VelnorV1TaskOwner, WorkspaceRecord,
};

use super::steps;
use crate::config::load_config;
use crate::config::private_wiring::PrivateWiring;
use crate::crate_jobs::crate_jobs_tests::discovery;
use crate::discover::{Discovery, PlannedWorkspace};
use crate::image_build_tasks::PrebuiltInputs;

const BASE: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"main\"\n";
const TABLE: &str = "[private_sources]\nowner = \"ChainArgos\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\napp_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\n";
const PRIVATE_LOCK: &str = "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n[[package]]\nname = \"shared\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/processor-shared?tag=v1#0123456789abcdef0123456789abcdef01234567\"\n";
const PLAIN_LOCK: &str = "[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n";

fn found() -> Discovery {
    let package = PackageRecord {
        id: "app 0.1.0".to_owned(),
        name: "app".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![TargetRecord {
            kind: "bin".to_owned(),
            name: "app".to_owned(),
            test: true,
            doctest: false,
            required_features: Vec::new(),
        }],
        features: Vec::new(),
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    };
    let mut found = discovery(Vec::new());
    found.workspaces = vec![PlannedWorkspace {
        record: WorkspaceRecord {
            workspace_root: String::new(),
            members: vec![package.id.clone()],
            packages: vec![package],
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
    }];
    found
}

fn image() -> ImageConfig {
    ImageConfig {
        id: "app".to_owned(),
        repository: "ghcr.io/chainargos/app".to_owned(),
        dockerfile: ImageDockerfile::Single("deploy/Dockerfile".to_owned()),
        context: ".".to_owned(),
        platforms: vec![ImagePlatform::LinuxAmd64],
        version_file: None,
        prebuilt: Some(ImagePrebuilt::CargoBin {
            package: "app".to_owned(),
            bin: "app".to_owned(),
            stage: "dist/app".to_owned(),
        }),
        timeout_minutes: 30,
    }
}

fn acquire() -> Step {
    Step {
        parallel: false,
        name: "Acquire Velnor".to_owned(),
        id: None,
        role: Some(StepRole::AcquireVelnor),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

/// Step names of the prebuilt block for a repository config and Cargo.lock.
fn names(config: &str, lock: &str) -> Vec<String> {
    let dir = tempfile::tempdir().expect("dir");
    fs::create_dir_all(dir.path().join(".velnor")).expect("velnor");
    fs::write(dir.path().join(".velnor/config.toml"), config).expect("config");
    fs::write(dir.path().join("Cargo.lock"), lock).expect("lock");
    let loaded = load_config(dir.path()).expect("config");
    let catalog = ToolCatalog::pinned();
    let private = PrivateWiring::load(dir.path(), &loaded, &catalog).expect("wiring");
    let found = found();
    let roots = [String::new()];
    let acquire = acquire();
    steps(
        &image(),
        &PrebuiltInputs {
            discovery: &found,
            catalog: &catalog,
            fetch_roots: &roots,
            acquire: Some(&acquire),
            private: private.as_ref(),
        },
    )
    .expect("prebuilt steps")
    .into_iter()
    .map(|step| step.name)
    .collect()
}

fn at(names: &[String], needle: &str) -> usize {
    names
        .iter()
        .position(|name| name == needle)
        .unwrap_or_else(|| panic!("{needle} missing from {names:?}"))
}

#[test]
fn private_cargo_sources_are_minted_fetched_and_revoked_before_the_cargo_fetch() {
    let names = names(&format!("{BASE}{TABLE}"), PRIVATE_LOCK);
    let acquire = at(&names, "Acquire Velnor");
    let mint = at(&names, "Mint read-only token for private sources");
    let fetch = at(&names, "Fetch private sources");
    let revoke = at(&names, "Revoke private-source token");
    let cargo = at(&names, "Fetch Cargo sources");
    let build = at(&names, "Build release binary");
    assert!(
        acquire < mint && mint < fetch && fetch < revoke && revoke < cargo && cargo < build,
        "{names:?}"
    );
}

#[test]
fn no_private_closure_or_no_table_adds_no_credential_step() {
    for (config, lock) in [
        (format!("{BASE}{TABLE}"), PLAIN_LOCK),
        (BASE.to_owned(), PRIVATE_LOCK),
    ] {
        let names = names(&config, lock);
        assert!(
            !names.iter().any(|name| name.contains("private")
                || name.contains("Mint")
                || name == "Acquire Velnor"),
            "{names:?}"
        );
        assert!(
            names.contains(&"Build release binary".to_owned()),
            "{names:?}"
        );
    }
}

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Schema-only consumer release manifest (debug builds skip the binary check).
fn manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "b".repeat(40)
    )
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = crate::git_fixture::command(dir)
        .expect("git command")
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "{args:?}");
}

/// Job steps of the full finalized workflow (setup, cache and token gates run).
fn finalized_image_job(lock: &str) -> Vec<String> {
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path();
    fs::create_dir_all(root.join(".velnor")).expect("velnor");
    fs::create_dir_all(root.join("deploy")).expect("deploy");
    let image = "[[images]]\nid = \"app\"\nrepository = \"ghcr.io/chainargos/app\"\ndockerfile = \"deploy/Dockerfile\"\nplatforms = [\"linux_amd64\"]\n[images.prebuilt]\nkind = \"cargo_bin\"\npackage = \"app\"\nbin = \"app\"\nstage = \"dist/app\"\n";
    fs::write(
        root.join(".velnor/config.toml"),
        format!("{BASE}{TABLE}{image}"),
    )
    .expect("config");
    fs::write(root.join("Cargo.lock"), lock).expect("lock");
    fs::write(
        root.join("deploy/Dockerfile"),
        format!("FROM debian:12@{DIGEST}\nCOPY dist/app /usr/local/bin/app\n"),
    )
    .expect("dockerfile");
    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    let config = load_config(root).expect("config");
    let mut found = found();
    found.consumer_manifest_json = Some(manifest_json());
    let workflow = crate::workflow::build_workflow(
        root,
        &config,
        "main",
        "ubuntu-26.04",
        &found,
        &[String::new()],
    )
    .expect("workflow");
    let mise = crate::pins::resolve_mise_setup(&config, "ubuntu-26.04").expect("mise");
    let jobs = velnor_actions_workflow_renderer::finalize_jobs(
        &workflow.ir,
        velnor_actions_contract::WorkflowPolicy::ConsumerV1,
        workflow.support.as_ref(),
        &workflow.context,
        &mise,
        &velnor_actions_workflow_renderer::WorkflowRoutingPolicy::hosted_only(),
    )
    .expect("finalized jobs");
    jobs["image-app-amd64"]
        .steps
        .iter()
        .map(|step| step.name.clone())
        .collect()
}

#[test]
fn the_finalized_image_job_keeps_the_block_between_helper_staging_and_the_build() {
    let names = finalized_image_job(PRIVATE_LOCK);
    let acquire = at(&names, "Acquire Velnor");
    let mint = at(&names, "Mint read-only token for private sources");
    let revoke = at(&names, "Revoke private-source token");
    let cargo = at(&names, "Fetch Cargo sources");
    let build = at(&names, "Build release binary");
    let docker = at(&names, "Build OCI image");
    assert!(
        acquire < mint && mint < revoke && revoke < cargo && cargo < build && build < docker,
        "{names:?}"
    );
    let plain = finalized_image_job(PLAIN_LOCK);
    assert!(
        !plain.iter().any(|name| name.contains("Mint")),
        "no private closure, no credential: {plain:?}"
    );
}
