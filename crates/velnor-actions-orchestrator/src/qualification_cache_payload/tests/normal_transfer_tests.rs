use super::*;

use std::fs;
use std::path::{Path, PathBuf};

const IDENTITY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    runner_temp: PathBuf,
    home: PathBuf,
    mise_data_dir: PathBuf,
    cargo_home: PathBuf,
    rustup_home: PathBuf,
    cache_key: String,
}

impl Fixture {
    fn new(parent: &Path, name: &str, state: &str) -> Self {
        let root = parent.join(name);
        fs::create_dir_all(&root).expect("fixture root");
        let root = fs::canonicalize(root).expect("canonical fixture root");
        let runner_temp = root.join("temp");
        let home = root.join("home");
        let mise_data_dir = home.join(".local/share/mise");
        let cargo_home = runner_temp.join("velnor/cargo");
        let rustup_home = runner_temp.join("velnor/rustup");
        let fixture = Self {
            runner_temp,
            home,
            mise_data_dir,
            cargo_home,
            rustup_home,
            cache_key: format!("mise-tools-v2-payload-v3-{IDENTITY}"),
        };
        fixture.write_state(state);
        fixture
    }

    fn request(&self) -> NormalToolsCachePayloadRequest<'_> {
        NormalToolsCachePayloadRequest {
            runner_temp: &self.runner_temp,
            home: &self.home,
            runtime_identity: IDENTITY,
            cache_key: &self.cache_key,
            mise_data_dir: &self.mise_data_dir,
            cargo_home: &self.cargo_home,
            rustup_home: &self.rustup_home,
        }
    }

    fn stage_path(&self) -> PathBuf {
        normal_tools_cache_stage_path(&self.runner_temp, &self.cache_key)
            .expect("checked stage path")
    }

    fn write_state(&self, state: &str) {
        fs::create_dir_all(self.cargo_home.join("bin")).expect("cargo bin");
        fs::create_dir_all(&self.mise_data_dir).expect("mise data");
        fs::create_dir_all(&self.rustup_home).expect("rustup home");
        fs::write(self.cargo_home.join("bin/cargo-tool"), state).expect("tool file");
        fs::write(self.cargo_home.join(".crates2.json"), state).expect("crate JSON");
        fs::write(self.cargo_home.join(".crates.toml"), state).expect("crate TOML");
        fs::write(self.mise_data_dir.join("tool-state"), state).expect("mise state");
        fs::write(self.rustup_home.join("rustup-state"), state).expect("rustup state");
    }

    fn tool_state(&self) -> String {
        fs::read_to_string(self.mise_data_dir.join("tool-state")).expect("mise state")
    }
}

fn copy_stage(source: &Path, destination: &Path) {
    fs::create_dir_all(destination.parent().expect("stage parent")).expect("create stage parent");
    transfer::payload::copy_tree_to_new_dir(source, destination).expect("copy restored stage");
}

#[test]
fn normal_stage_path_hashes_the_full_v3_key_and_rejects_old_namespaces() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let fixture = Fixture::new(temp.path(), "runner", "state");
    let path = normal_tools_cache_stage_path(&fixture.runner_temp, &fixture.cache_key)
        .expect("stage path");
    let digest = blake3::hash(fixture.cache_key.as_bytes()).to_hex();

    assert_eq!(
        path,
        fixture.runner_temp.join(format!(
            "velnor/normal-cache-v3/mise_tools/{digest}/payload"
        ))
    );
    assert_eq!(path, fixture.stage_path());
    assert_eq!(
        normal_tools_cache_stage_path(
            &fixture.runner_temp,
            &format!("mise-tools-v2-{}", "a".repeat(64))
        ),
        Err("normal_cache_key_invalid")
    );
    assert_eq!(
        normal_tools_cache_stage_path(Path::new("/runner/../temp"), &fixture.cache_key),
        Err("normal_cache_stage_path_invalid")
    );
}

#[test]
fn stage_path_does_not_prepare_or_delete_an_action_target() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let fixture = Fixture::new(temp.path(), "runner", "live-state");
    let stage = normal_tools_cache_stage_path(&fixture.runner_temp, &fixture.cache_key)
        .expect("derive action stage path");
    assert_eq!(stage, fixture.stage_path());
    assert!(!stage.exists());

    let miss = import_normal_tools_cache_payload(
        &fixture.request(),
        &CacheActionObservation {
            hit: false,
            matched_key: None,
        },
    )
    .expect("cold absence");
    assert_eq!(miss.status, CacheRestoreStatus::Miss);
    assert!(!stage.exists());
    assert_eq!(fixture.tool_state(), "live-state");

    fs::create_dir_all(&stage).expect("preexisting stage");
    let marker = stage.join("foreign-marker");
    fs::write(&marker, b"leave this file unchanged").expect("foreign marker");
    assert_eq!(
        import_normal_tools_cache_payload(
            &fixture.request(),
            &CacheActionObservation {
                hit: false,
                matched_key: None,
            },
        ),
        Err("normal_cache_restore_unexpected_stage_on_miss")
    );
    assert_eq!(
        fs::read(&marker).expect("foreign marker after rejected preparation"),
        b"leave this file unchanged"
    );
}

#[test]
fn cold_miss_keeps_live_roots_and_warm_import_restores_the_exported_state() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let cold = Fixture::new(temp.path(), "cold", "cold-state");
    let export = export_normal_tools_cache_payload(&cold.request()).expect("export payload");
    assert_eq!(export.save_key.as_deref(), Some(cold.cache_key.as_str()));
    assert_eq!(
        export.save_stage_path.as_deref(),
        Some(cold.stage_path().as_path())
    );

    let absent = Fixture::new(temp.path(), "absent", "absent-state");
    let miss = import_normal_tools_cache_payload(
        &absent.request(),
        &CacheActionObservation {
            hit: false,
            matched_key: None,
        },
    )
    .expect("cold absence");
    assert_eq!(miss.status, CacheRestoreStatus::Miss);
    assert_eq!(absent.tool_state(), "absent-state");

    let warm = Fixture::new(temp.path(), "warm", "warm-state");
    copy_stage(&cold.stage_path(), &warm.stage_path());
    let restore = import_normal_tools_cache_payload(
        &warm.request(),
        &CacheActionObservation {
            hit: true,
            matched_key: Some(warm.cache_key.clone()),
        },
    )
    .expect("warm import");
    assert_eq!(restore.status, CacheRestoreStatus::Hit);
    assert_eq!(
        restore.state_digest.as_deref(),
        Some(export.state_digest.as_str())
    );
    assert_eq!(warm.tool_state(), "cold-state");
    assert_eq!(
        fs::read(warm.cargo_home.join("bin/cargo-tool")).expect("tool bytes"),
        b"cold-state"
    );
}

#[test]
fn malformed_stage_is_removed_without_changing_live_roots() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let cold = Fixture::new(temp.path(), "cold", "saved-state");
    export_normal_tools_cache_payload(&cold.request()).expect("export payload");

    let warm = Fixture::new(temp.path(), "warm", "live-state");
    copy_stage(&cold.stage_path(), &warm.stage_path());
    fs::write(warm.stage_path().join("manifest.json"), b"{\"schema\":2}")
        .expect("corrupt manifest");
    let unrelated = warm
        .stage_path()
        .parent()
        .expect("stage parent")
        .join("keep");
    fs::write(&unrelated, "keep").expect("unrelated file");

    let result = import_normal_tools_cache_payload(
        &warm.request(),
        &CacheActionObservation {
            hit: true,
            matched_key: Some(warm.cache_key.clone()),
        },
    );
    assert!(result.is_err());
    assert_eq!(warm.tool_state(), "live-state");
    assert_eq!(
        fs::read(warm.cargo_home.join("bin/cargo-tool")).expect("tool bytes"),
        b"live-state"
    );
    assert!(!warm.stage_path().exists());
    assert_eq!(
        fs::read_to_string(unrelated).expect("unrelated file"),
        "keep"
    );
}
