// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use sha1_checked::{Digest as _, Sha1};
use sha2::{Digest as _, Sha256};
use std::{
    error::Error,
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REPO: &str = "repos/tailrocks/velnor-new";
const FAMILY: &str = "\n[runner_image_family]\nmac_qualification = { runner_id = 2, companion_workflow_path = \".github/workflows/qualify-runner-family.yml\", companion_workflow_ref = \"refs/heads/main\", companion_workflow_sha = \"cccccccccccccccccccccccccccccccccccccccc\", docker_socket = \"unix:///tmp/fixture.sock\", state_dir = \"/tmp/fixture-state\", sdk = { channel = \"stable\", version = \"0.1.78\", source_sha = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\", manifest_sha256 = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\" } }\n";

pub(super) struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    pub state: Value,
    pub files: Vec<(String, Vec<u8>)>,
    pub context: Value,
}
impl Fixture {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let assets = setup(&root)?;
        let publisher_zip = archive(
            &root,
            "publisher.zip",
            &assets,
            &[
                "RUNNER_IMAGE_MANIFEST.json",
                "SHA256SUMS",
                "LICENSE",
                "NOTICE",
            ],
        )?;
        let lock =
            fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.velnor/runner.lock"))?;
        let mut workflow = Vec::new();
        GzDecoder::new(include_bytes!("fixtures/context-workflow.yml.gz").as_slice())
            .read_to_end(&mut workflow)?;
        let context = json!({"schema":"velnor.runner-image-publication-context.v1", "repository":"tailrocks/velnor-new", "repository_id":1_390_620_900_u64, "source_commit":SOURCE, "public_run_id":200, "public_run_attempt":2, "producer_run_id":100, "producer_run_attempt":1, "producer_artifact_id":300, "publisher_artifact_id":400, "publisher_artifact_digest":sha(&publisher_zip), "manifest_sha256":sha(&fs::read(assets.join("RUNNER_IMAGE_MANIFEST.json"))?), "runner_lock_sha256":sha(&lock)});
        let repo = json!({"id":1_390_620_900_u64,"full_name":"tailrocks/velnor-new","default_branch":"main"});
        let run = |id, attempt, path| json!({"id":id,"run_number":10,"run_attempt":attempt,"event":"push","path":path,"head_branch":"main","head_sha":SOURCE,"head_repository":repo,"status":"completed","conclusion":"success"});
        let mut public = run(200, 2, ".github/workflows/product-release-images.yml");
        public["status"] = json!("in_progress");
        public["conclusion"] = Value::Null;
        let job = |id, name, run, attempt| json!({"id":id,"name":name,"run_id":run,"run_attempt":attempt,"head_sha":SOURCE,"status":"completed","conclusion":"success"});
        let producer_artifact = json!({"id":300,"name":"image-assets-1","expired":false,"size_in_bytes":123,"workflow_run":{"id":100,"head_sha":SOURCE,"head_branch":"main"}});
        let mut api = json!({});
        api[REPO] = repo.clone();
        api[format!("{REPO}/branches/main")] =
            json!({"name":"main","protected":true,"commit":{"sha":SOURCE}});
        api[format!("{REPO}/actions/runs/200/attempts/2")] = public;
        api[format!("{REPO}/actions/runs/200/attempts/2/jobs")] =
            json!({"total_count":1,"jobs":[job(201,"publish-images",200,2)]});
        api[format!("{REPO}/actions/runs/100/attempts/1")] =
            run(100, 1, ".github/workflows/ci.yml");
        api[format!("{REPO}/actions/runs/100/attempts/1/jobs")] =
            json!({"total_count":1,"jobs":[job(101,"Required",100,1)]});
        api[format!("{REPO}/actions/runs/100/artifacts")] =
            json!({"total_count":1,"artifacts":[producer_artifact]});
        api[format!("{REPO}/actions/artifacts/400")] =
            artifact(400, "runner-image-release-2", 200, &publisher_zip);
        api[format!("{REPO}/actions/artifacts/400/zip")] =
            json!({"raw_base64":STANDARD.encode(&publisher_zip)});
        let config = format!(
            "{}{}",
            fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.velnor/config.toml")
            )?,
            FAMILY
        );
        let generator =
            fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.velnor/generator.lock"))?;
        let files = vec![
            (".velnor/config.toml".to_owned(), config.into_bytes()),
            (".velnor/generator.lock".to_owned(), generator),
            (".velnor/runner.lock".to_owned(), lock),
            (
                ".github/workflows/product-release-images.yml".to_owned(),
                workflow,
            ),
        ];
        source_api(&mut api, &files)?;
        let mut fixture = Self {
            _temp: temp,
            root,
            state: json!({"context_api":api}),
            files,
            context,
        };
        fixture.update_context()?;
        Ok(fixture)
    }
    pub(super) fn update_context(&mut self) -> Result<(), Box<dyn Error>> {
        self.update_context_raw(&serde_json::to_vec(&self.context)?)
    }
    pub(super) fn update_context_raw(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        let path = self.root.join("runner-image-publication-context.json");
        fs::write(&path, data)?;
        let bytes = archive(
            &self.root,
            "context.zip",
            &self.root,
            &["runner-image-publication-context.json"],
        )?;
        self.set_context_archive(bytes);
        Ok(())
    }
    fn set_context_archive(&mut self, bytes: Vec<u8>) {
        let value = artifact(500, "runner-image-publication-context-2", 200, &bytes);
        let api = &mut self.state["context_api"];
        api[format!("{REPO}/actions/runs/200/artifacts")] =
            json!({"total_count":1,"artifacts":[value]});
        api[format!("{REPO}/actions/artifacts/500")] = value;
        api[format!("{REPO}/actions/artifacts/500/zip")] =
            json!({"raw_base64":STANDARD.encode(bytes)});
    }
    pub(super) fn wrong_context_archive(&mut self, extra: bool) -> Result<(), Box<dyn Error>> {
        fs::write(self.root.join("extra"), b"unexpected")?;
        let names = if extra {
            vec!["runner-image-publication-context.json", "extra"]
        } else {
            vec!["extra"]
        };
        let bytes = archive(&self.root, "bad.zip", &self.root, &names)?;
        self.set_context_archive(bytes);
        Ok(())
    }
    pub(super) fn symlink_context_archive(&mut self) -> Result<(), Box<dyn Error>> {
        let path = self.root.join("runner-image-publication-context.json");
        fs::remove_file(&path)?;
        fs::write(
            self.root.join("symlink-target"),
            serde_json::to_vec(&self.context)?,
        )?;
        std::os::unix::fs::symlink("symlink-target", &path)?;
        let bytes = archive(
            &self.root,
            "link.zip",
            &self.root,
            &["runner-image-publication-context.json"],
        )?;
        self.set_context_archive(bytes);
        fs::remove_file(path)?;
        Ok(())
    }
    pub(super) fn duplicate_context_archive(&mut self) -> Result<(), Box<dyn Error>> {
        let data = serde_json::to_vec(&self.context)?;
        let name = "runner-image-publication-context.json";
        let bytes = super::stored_zip::archive(&[(name, &data), (name, &data)])?;
        self.set_context_archive(bytes);
        Ok(())
    }
    pub(super) fn duplicate_publisher_archive(&mut self) -> Result<(), Box<dyn Error>> {
        let assets = self.root.join("assets");
        let names = [
            "RUNNER_IMAGE_MANIFEST.json",
            "SHA256SUMS",
            "LICENSE",
            "NOTICE",
            "RUNNER_IMAGE_MANIFEST.json",
        ];
        let contents: Vec<_> = names
            .iter()
            .map(|name| fs::read(assets.join(name)))
            .collect::<Result<_, _>>()?;
        let entries: Vec<_> = names
            .iter()
            .zip(&contents)
            .map(|(name, data)| (*name, data.as_slice()))
            .collect();
        let bytes = super::stored_zip::archive(&entries)?;
        self.context["publisher_artifact_digest"] = json!(sha(&bytes));
        self.state["context_api"][format!("{REPO}/actions/artifacts/400")] =
            artifact(400, "runner-image-release-2", 200, &bytes);
        self.state["context_api"][format!("{REPO}/actions/artifacts/400/zip")] =
            json!({"raw_base64":STANDARD.encode(bytes)});
        self.update_context()
    }
    pub(super) fn set_workflow(&mut self, bytes: Vec<u8>) -> Result<(), Box<dyn Error>> {
        self.files
            .iter_mut()
            .find(|(path, _)| path == ".github/workflows/product-release-images.yml")
            .ok_or("workflow")?
            .1 = bytes;
        source_api(&mut self.state["context_api"], &self.files)
    }
    pub(super) fn write_asset(&self, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        fs::write(self.root.join("assets").join(name), bytes)?;
        Ok(())
    }
    pub(super) fn error(&self) -> String {
        fs::read_to_string(self.root.join("home/image-publication-fixture-error"))
            .unwrap_or_default()
    }
    pub(super) fn invoke(&self) -> Result<Output, Box<dyn Error>> {
        fs::write(
            self.root.join("home/fixture-state.json"),
            serde_json::to_vec(&self.state)?,
        )?;
        Ok(Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
            .env_clear()
            .env("PATH", self.root.join("bin"))
            .env("HOME", self.root.join("home"))
            .env("GH_TOKEN", "fixture-token")
            .args([
                "image-maintenance",
                "admit-mac-publication",
                "--source-commit",
                SOURCE,
                "--release-run-id",
                "200",
                "--release-run-attempt",
                "2",
                "--assets-dir",
            ])
            .arg(self.root.join("assets"))
            .arg("--runner-temp")
            .arg(&self.root)
            .output()?)
    }
}
fn setup(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let home = root.join("home");
    fs::create_dir(&home)?;
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let fixture =
        std::env::var_os("VELNOR_IMAGE_PUBLICATION_FIXTURE").ok_or("fixture executable missing")?;
    fs::copy(fixture, bin.join("mise"))?;
    fs::set_permissions(bin.join("mise"), fs::Permissions::from_mode(0o755))?;
    let assets = root.join("assets");
    fs::create_dir(&assets)?;
    for (name, bytes) in [
        ("RUNNER_IMAGE_MANIFEST.json", b"{\"schema\":1}\n".as_slice()),
        ("SHA256SUMS", b"sums\n"),
        ("LICENSE", b"license\n"),
        ("NOTICE", b"notice\n"),
    ] {
        fs::write(assets.join(name), bytes)?;
    }
    Ok(assets)
}
fn archive(root: &Path, name: &str, cwd: &Path, names: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let path = root.join(name);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    if !Command::new("/usr/bin/zip")
        .current_dir(cwd)
        .arg("-qy")
        .arg(&path)
        .args(names)
        .status()?
        .success()
    {
        return Err("zip failed".into());
    }
    Ok(fs::read(path)?)
}
fn sha(bytes: &[u8]) -> String {
    format!("sha256:{}", super::support::hex(&Sha256::digest(bytes)))
}
fn artifact(id: u64, name: &str, run: u64, bytes: &[u8]) -> Value {
    json!({"id":id,"name":name,"expired":false,"size_in_bytes":bytes.len(),"digest":sha(bytes),"workflow_run":{"id":run,"head_sha":SOURCE,"head_branch":"main"}})
}
fn source_api(api: &mut Value, files: &[(String, Vec<u8>)]) -> Result<(), Box<dyn Error>> {
    let roots = [
        ("", "1".repeat(40)),
        (".velnor", "2".repeat(40)),
        (".github", "3".repeat(40)),
        (".github/workflows", "4".repeat(40)),
    ];
    api[format!("{REPO}/git/commits/{SOURCE}")] = json!({"sha":SOURCE,"tree":{"sha":roots[0].1}});
    for (dir, id) in &roots {
        let mut entries = Vec::new();
        for (sub, child) in &roots {
            if !sub.is_empty() && sub.rsplit_once('/').map_or("", |(parent, _)| parent) == *dir {
                entries.push(json!({"path":sub.rsplit('/').next().ok_or("dir")?,"mode":"040000","type":"tree","sha":child}));
            }
        }
        for (path, data) in files {
            let (parent, name) = path.rsplit_once('/').ok_or("path")?;
            if parent == *dir {
                let mut hasher = Sha1::new();
                hasher.update(format!("blob {}\0", data.len()));
                hasher.update(data);
                let digest = hasher.try_finalize();
                if digest.has_collision() {
                    return Err("sha collision".into());
                }
                let blob = super::support::hex(digest.hash());
                entries.push(json!({"path":name,"mode":"100644","type":"blob","sha":blob}));
                api[format!("{REPO}/git/blobs/{blob}")] = json!({"sha":blob,"encoding":"base64","size":data.len(),"content":STANDARD.encode(data)});
            }
        }
        api[format!("{REPO}/git/trees/{id}")] = json!({"sha":id,"truncated":false,"tree":entries});
    }
    Ok(())
}
