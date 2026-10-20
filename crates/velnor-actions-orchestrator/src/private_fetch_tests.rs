use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;

use velnor_actions_contract::deps_lock::{DEPS_LOCK_SCHEMA, DepsLock, LockedAsset, LockedGitTree};

use super::*;

const OWNER: &str = "ChainArgos";
const EXPLORER_COMMIT: &str = "1111111111111111111111111111111111111111";
const SHARED_COMMIT: &str = "2222222222222222222222222222222222222222";
const MONO_COMMIT: &str = "3333333333333333333333333333333333333333";

fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *data)
            .expect("append");
    }
    gzip(&builder.into_inner().expect("tar"))
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(raw).expect("gzip");
    encoder.finish().expect("finish")
}

/// A tar whose single entry carries a raw (unvalidated) path.
fn hostile_tar_gz(path: &str, link: Option<&str>) -> Vec<u8> {
    let mut header = tar::Header::new_gnu();
    {
        let old = header.as_old_mut();
        old.name[..path.len()].copy_from_slice(path.as_bytes());
    }
    header.set_size(if link.is_some() { 0 } else { 4 });
    header.set_mode(0o644);
    if let Some(target) = link {
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_link_name(target).expect("link name");
    }
    header.set_cksum();
    let mut raw = Vec::new();
    raw.extend_from_slice(header.as_bytes());
    if link.is_none() {
        raw.extend_from_slice(b"evil");
        raw.extend_from_slice(&[0_u8; 508]);
    }
    raw.extend_from_slice(&[0_u8; 1024]);
    gzip(&raw)
}

#[derive(Default)]
struct Fake {
    routes: RefCell<BTreeMap<String, Vec<u8>>>,
    downloads: RefCell<Vec<String>>,
    cargo: RefCell<Vec<PathBuf>>,
}

impl Fake {
    fn serve(&self, route: &str, body: impl Into<Vec<u8>>) {
        self.routes
            .borrow_mut()
            .insert(route.to_owned(), body.into());
    }
}

impl FetchBackend for Fake {
    fn cargo_fetch(&self, manifest: &Path) -> Result<(), OrchestratorError> {
        self.cargo.borrow_mut().push(manifest.to_owned());
        Ok(())
    }

    fn download(&self, route: &str, _accept: &str, dest: &Path) -> Result<(), OrchestratorError> {
        self.downloads.borrow_mut().push(route.to_owned());
        let body = self
            .routes
            .borrow()
            .get(route)
            .cloned()
            .ok_or_else(|| internal(&format!("fake_missing_route:{route}")))?;
        fs::write(dest, body).map_err(|e| internal(&e.to_string()))
    }
}

struct World {
    workspace: tempfile::TempDir,
    /// Keeps the runner-temp directory alive; `temp` is its canonical path.
    _temp_guard: tempfile::TempDir,
    temp: Temp,
    fake: Fake,
    schema_archive: Vec<u8>,
}

/// Canonical runner-temp path (the archive reader refuses symlinked parents).
struct Temp(PathBuf);

impl Temp {
    fn path(&self) -> &Path {
        &self.0
    }
}

fn sha(bytes: &[u8]) -> String {
    crate::generator::sha256_hex(bytes)
}

fn release_json(id: u64, name: &str, bytes: &[u8]) -> String {
    format!(
        "{{\"draft\":false,\"assets\":[{{\"id\":{id},\"name\":\"{name}\",\"state\":\"uploaded\",\"digest\":\"sha256:{}\"}}]}}",
        sha(bytes)
    )
}

const CONFIG: &str = r#"schema = 1
[workflow]
name = "CI"
[private_sources]
owner = "ChainArgos"
app_id_variable = "VELNOR_DEPS_APP_ID"
app_private_key_secret = "VELNOR_DEPS_APP_PRIVATE_KEY"
[[private_sources.maven_bundles]]
id = "libs"
repository = "ChainArgos/processor-shared"
tag = "libs-v1"
asset = "libs-1-maven.tar.gz"
[[private_sources.release_assets]]
id = "schema"
repository = "ChainArgos/blockchain-explorer"
tag = "schema-v1"
asset = "schema-1.tar.gz"
extract_to = "explorer-schema"
[[private_sources.git_trees]]
id = "history"
repository = "ChainArgos/java-monorepo"
tag = "platform-history/2025-11"
"#;

fn locked(
    id: &str,
    repository: &str,
    tag: &str,
    asset: &str,
    archive: &[u8],
    commit: &str,
) -> LockedAsset {
    LockedAsset {
        id: id.to_owned(),
        repository: repository.to_owned(),
        tag: tag.to_owned(),
        commit: commit.to_owned(),
        asset: asset.to_owned(),
        sha256: sha(archive),
        extract_to: None,
    }
}

fn serve_release(
    fake: &Fake,
    repository: &str,
    tag: &str,
    commit: &str,
    asset: (u64, &str, &[u8]),
) {
    fake.serve(
        &format!("repos/{repository}/commits/refs/tags/{tag}"),
        format!("{{\"sha\":\"{commit}\"}}"),
    );
    fake.serve(
        &format!("repos/{repository}/releases/tags/{tag}"),
        release_json(asset.0, asset.1, asset.2),
    );
    fake.serve(
        &format!("repos/{repository}/releases/assets/{}", asset.0),
        asset.2,
    );
}

fn world() -> World {
    let workspace = tempfile::tempdir().expect("workspace");
    let temp = tempfile::tempdir().expect("temp");
    let schema_archive = tar_gz(&[("schema.json", b"{\"v\":1}"), ("sub/readme.md", b"hi")]);
    let maven_archive = tar_gz(&[("com/acme/lib/1/lib-1.jar", b"jar")]);
    let tree_archive = tar_gz(&[("ChainArgos-java-monorepo-3333/file.txt", b"tree")]);
    let root = workspace.path();
    fs::create_dir_all(root.join(".velnor")).expect("velnor");
    fs::write(root.join(".velnor/config.toml"), CONFIG).expect("config");
    let mut schema = locked(
        "schema",
        "ChainArgos/blockchain-explorer",
        "schema-v1",
        "schema-1.tar.gz",
        &schema_archive,
        EXPLORER_COMMIT,
    );
    schema.extract_to = Some("explorer-schema".to_owned());
    let lock = DepsLock {
        schema: DEPS_LOCK_SCHEMA,
        maven_bundles: vec![locked(
            "libs",
            "ChainArgos/processor-shared",
            "libs-v1",
            "libs-1-maven.tar.gz",
            &maven_archive,
            SHARED_COMMIT,
        )],
        release_assets: vec![schema],
        git_trees: vec![LockedGitTree {
            id: "history".to_owned(),
            repository: "ChainArgos/java-monorepo".to_owned(),
            tag: "platform-history/2025-11".to_owned(),
            commit: MONO_COMMIT.to_owned(),
        }],
    };
    fs::write(
        root.join(".velnor/deps.lock"),
        crate::config::deps_lock::render_lock(&lock).expect("lock"),
    )
    .expect("lock file");
    fs::write(
        root.join("Cargo.lock"),
        "[[package]]\nname = \"d\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/eth-domain?tag=v1#4444444444444444444444444444444444444444\"\n",
    )
    .expect("cargo lock");
    let fake = Fake::default();
    serve_release(
        &fake,
        "ChainArgos/blockchain-explorer",
        "schema-v1",
        EXPLORER_COMMIT,
        (42, "schema-1.tar.gz", &schema_archive),
    );
    serve_release(
        &fake,
        "ChainArgos/processor-shared",
        "libs-v1",
        SHARED_COMMIT,
        (43, "libs-1-maven.tar.gz", &maven_archive),
    );
    fake.serve(
        &format!("repos/ChainArgos/java-monorepo/tarball/{MONO_COMMIT}"),
        tree_archive,
    );
    let canonical = temp.path().canonicalize().expect("canonical temp");
    World {
        workspace,
        _temp_guard: temp,
        temp: Temp(canonical),
        fake,
        schema_archive,
    }
}

fn request(world: &World, repositories: &[&str], kinds: &[SourceKind]) -> FetchRequest {
    FetchRequest {
        owner: OWNER.to_owned(),
        repositories: repositories.iter().map(|r| (*r).to_owned()).collect(),
        kinds: kinds.iter().copied().collect(),
        roots: vec![String::new()],
        workspace: world.workspace.path().to_owned(),
        runner_temp: world.temp.path().to_owned(),
    }
}

const ALL: [SourceKind; 4] = [
    SourceKind::Cargo,
    SourceKind::GitTrees,
    SourceKind::MavenBundles,
    SourceKind::ReleaseAssets,
];
const ALL_REPOS: [&str; 4] = [
    "blockchain-explorer",
    "eth-domain",
    "java-monorepo",
    "processor-shared",
];

fn problem(result: Result<FetchSummary, OrchestratorError>) -> String {
    result.map_or_else(
        |error| error.to_string(),
        |summary| format!("ok {summary:?}"),
    )
}

#[test]
fn a_full_fetch_pins_extracts_and_runs_cargo_fetch_locked_per_root() {
    let world = world();
    let request = request(&world, &ALL_REPOS, &ALL);
    let summary = execute(&request, &world.fake).expect("fetch");
    assert_eq!(
        summary,
        FetchSummary {
            cargo_roots: 1,
            assets: 2,
            trees: 1
        }
    );
    assert_eq!(
        *world.fake.cargo.borrow(),
        [world.workspace.path().join("Cargo.toml")]
    );
    let deps = world.temp.path().join("velnor/deps");
    assert_eq!(
        fs::read(deps.join("explorer-schema/schema.json")).expect("schema"),
        b"{\"v\":1}"
    );
    assert!(deps.join("explorer-schema/sub/readme.md").is_file());
    assert!(deps.join("maven/libs/com/acme/lib/1/lib-1.jar").is_file());
    assert!(
        deps.join("git/history/ChainArgos-java-monorepo-3333/file.txt")
            .is_file()
    );
    assert!(
        !world.temp.path().join("velnor/private-downloads").exists(),
        "downloaded archives are removed"
    );
    let downloads = world.fake.downloads.borrow();
    let tag_check = downloads
        .iter()
        .position(|r| r.ends_with("commits/refs/tags/schema-v1"))
        .expect("tag check");
    let asset = downloads
        .iter()
        .position(|r| r.ends_with("releases/assets/42"))
        .expect("asset");
    assert!(
        tag_check < asset,
        "the moved-tag check precedes the download"
    );
}

#[test]
fn kinds_not_requested_are_not_fetched() {
    let world = world();
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    let summary = execute(&request, &world.fake).expect("fetch");
    assert_eq!(summary.cargo_roots + summary.trees, 0);
    assert!(world.fake.cargo.borrow().is_empty());
    assert!(
        !world
            .fake
            .downloads
            .borrow()
            .iter()
            .any(|r| r.contains("processor-shared") || r.contains("java-monorepo"))
    );
}

#[path = "private_fetch_attack_tests.rs"]
mod attack;
#[path = "private_fetch_env_tests.rs"]
mod env_parse;
