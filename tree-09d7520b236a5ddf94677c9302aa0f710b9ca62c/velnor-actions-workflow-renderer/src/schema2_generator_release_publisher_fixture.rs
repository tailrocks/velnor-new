use super::super::{
    LINUX_TARGET, MACOS_TARGET, MANIFEST_NAME, RELEASE_VERSION, Scratch, assert_success,
    asset_record, asset_records, asset_url, output_text, release_json, write_local_assets,
};
use std::env;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const REPOSITORY: &str = "tailrocks/velnor-new";
const FAKE_MISE: &str = r##"#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["GH_FAKE_MISE_CALLS"], "a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\n")
if len(args) < 4 or args[3] not in ("install", "exec"):
    raise SystemExit(97)
if args[3] == "install":
    if any(os.environ.get(name) for name in ("GH_TOKEN", "GITHUB_TOKEN", "MISE_GITHUB_TOKEN")):
        raise SystemExit(99)
    raise SystemExit(0)
if args[4:6] != ["gh@2.102.0", "--"]:
    raise SystemExit(98)
if not os.environ.get("GH_TOKEN"):
    raise SystemExit(89)
os.environ["GH_FAKE_MISE_EXECUTED"] = "1"
os.execvpe(args[6], args[6:], os.environ)
"##;

const FAKE_GH: &str = r##"#!/usr/bin/env python3
import json, os, shutil, sys
from pathlib import Path
args = sys.argv[1:]
if os.environ.get("GH_FAKE_MISE_EXECUTED") != "1":
    raise SystemExit(88)
with open(os.environ["GH_FAKE_CALLS"], "a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\n")
if args[0] == "api":
    endpoint_index = args.index("api") + 1
    endpoint = args[endpoint_index]
    if endpoint == "repos/tailrocks/velnor-new/commits/main":
        result = {"sha": os.environ["GH_FAKE_MAIN_SHA"]}
    elif endpoint == "repos/tailrocks/velnor-new/git/refs" and args[args.index("--method") + 1] == "POST":
        fields = [args[index + 1] for index, value in enumerate(args) if value == "-f"]
        values = dict(value.split("=", 1) for value in fields)
        if values.get("sha") != os.environ["GITHUB_SHA"]:
            raise SystemExit(86)
        reference = values.get("ref", "")
        tag = reference.rsplit("/", 1)[-1]
        Path(os.environ["GH_FAKE_TAG_PATH"]).write_text(tag, encoding="utf-8")
        result = {"ref": reference, "object": {"type": "commit", "sha": values["sha"]}}
    elif "/git/ref/tags/" in endpoint:
        tag = endpoint.rsplit("/", 1)[1]
        tag_path = Path(os.environ["GH_FAKE_TAG_PATH"])
        if not tag_path.exists() or tag_path.read_text(encoding="utf-8") != tag:
            raise SystemExit(95)
        result = {"ref": f"refs/tags/{tag}", "object": {"type": "commit", "sha": os.environ["GH_FAKE_TAG_SHA"]}}
    elif "/releases/tags/" in endpoint:
        raise SystemExit(96)
    elif endpoint == "repos/tailrocks/velnor-new/releases/741852963":
        if not Path(os.environ["GH_FAKE_RELEASE_CREATED"]).exists():
            raise SystemExit(96)
        if Path(os.environ["GH_FAKE_ACCEPTED_DIRECTORY"]).exists():
            raise SystemExit(83)
        index_file = Path(os.environ["GH_FAKE_RELEASE_INDEX"])
        index = int(index_file.read_text()) if index_file.exists() else 0
        index_file.write_text(str(index + 1))
        response = Path(os.environ["GH_FAKE_API_DIR"]) / f"release-{index}.json"
        result = json.loads(response.read_text(encoding="utf-8"))
    else:
        raise SystemExit(96)
    print(json.dumps(result, separators=(",", ":")))
elif args[0] == "release" and args[1] == "upload":
    if Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).exists():
        raise SystemExit(81)
    if Path(os.environ["GH_FAKE_ACCEPTED_DIRECTORY"]).exists():
        raise SystemExit(83)
    if args[args.index("--repo") + 1] != "tailrocks/velnor-new":
        raise SystemExit(90)
    uploaded = []
    index = 3
    while index < len(args):
        value = args[index]
        if value == "--repo":
            break
        path = Path(value)
        if not path.is_file():
            raise SystemExit(95)
        if path.name == "velnor-actions-release-manifest.json":
            shutil.copyfile(path, os.environ["GH_FAKE_MANIFEST_COPY"])
        if path.name == "velnor-actions-release-manifest.json.sha256":
            shutil.copyfile(path, os.environ["GH_FAKE_MANIFEST_CHECKSUM_COPY"])
        uploaded.append(path.name)
        index += 1
    with open(os.environ["GH_FAKE_UPLOADS"], "a", encoding="utf-8") as log:
        log.write("draft-upload:" + ",".join(uploaded) + "\n")
elif args[0] == "release" and args[1] in ("create", "edit"):
    if args[1] == "create":
        if args[args.index("--repo") + 1] != "tailrocks/velnor-new":
            raise SystemExit(90)
        target = args[args.index("--target") + 1]
        if target != os.environ["GITHUB_SHA"] or args[2] != "generator-" + target:
            raise SystemExit(93)
        if "--draft" not in args or "--latest=false" not in args:
            raise SystemExit(92)
        tag_path = Path(os.environ["GH_FAKE_TAG_PATH"])
        if not tag_path.exists() or tag_path.read_text(encoding="utf-8") != args[2]:
            raise SystemExit(87)
        Path(os.environ["GH_FAKE_RELEASE_CREATED"]).write_text(args[2], encoding="utf-8")
    else:
        if args[args.index("--repo") + 1] != "tailrocks/velnor-new":
            raise SystemExit(90)
        if "--draft=false" not in args:
            raise SystemExit(91)
        if Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).exists():
            raise SystemExit(82)
        if Path(os.environ["GH_FAKE_ACCEPTED_DIRECTORY"]).exists():
            raise SystemExit(83)
        Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).write_text("true", encoding="utf-8")
        with open(os.environ["GH_FAKE_UPLOADS"], "a", encoding="utf-8") as log:
            log.write("publish\n")
elif args[0] == "release" and args[1] == "view":
    tag = args[2]
    created = Path(os.environ["GH_FAKE_RELEASE_CREATED"])
    if not created.exists() or created.read_text(encoding="utf-8") != tag:
        raise SystemExit(84)
    print(json.dumps({"databaseId": 741852963, "tagName": tag, "isDraft": not Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).exists()}))
else:
    raise SystemExit(94)
"##;

pub(super) struct PublisherFixture {
    _scratch: Scratch,
    workspace: PathBuf,
    assets: PathBuf,
    api: PathBuf,
    bin: PathBuf,
    runner_temp: PathBuf,
    pub(super) calls: PathBuf,
    pub(super) mise_calls: PathBuf,
    pub(super) release_index: PathBuf,
    pub(super) tag_path: PathBuf,
    pub(super) release_created: PathBuf,
    pub(super) release_published: PathBuf,
    pub(super) manifest_copy: PathBuf,
    pub(super) manifest_checksum_copy: PathBuf,
    pub(super) accepted_directory: PathBuf,
    pub(super) upload_log: PathBuf,
    pub(super) commit: String,
    pub(super) tag: String,
}

impl PublisherFixture {
    pub(super) fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let scratch = Scratch::new(name)?;
        let workspace = scratch.path().join("workspace");
        let assets = scratch.path().join("assets");
        let api = scratch.path().join("api");
        let bin = scratch.path().join("bin");
        let runner_temp = scratch.path().join("runner-temp");
        for directory in [&workspace, &assets, &api, &bin, &runner_temp] {
            fs::create_dir_all(directory)?;
        }
        let calls = scratch.path().join("gh-calls.jsonl");
        let mise_calls = scratch.path().join("mise-calls.jsonl");
        let release_index = scratch.path().join("release-index");
        let tag_path = scratch.path().join("tag-ref");
        let release_created = scratch.path().join("release-created");
        let release_published = scratch.path().join("release-published");
        let manifest_copy = scratch.path().join("uploaded-manifest.json");
        let manifest_checksum_copy = scratch.path().join("uploaded-manifest.sha256");
        let accepted_directory = runner_temp.join("velnor-generator-accepted");
        let upload_log = scratch.path().join("uploads.txt");
        let output = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        fs::write(workspace.join("source.txt"), b"fixture source\n")?;
        let output = Command::new("git")
            .args(["add", "source.txt"])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "fixture source",
            ])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        let commit = String::from_utf8(output.stdout)?.trim().to_owned();
        let tag = format!("generator-{commit}");
        write_local_assets(&assets, RELEASE_VERSION)?;
        copy_build_assets(&assets, &workspace)?;
        write_executable(&bin.join("mise"), FAKE_MISE)?;
        write_executable(&bin.join("gh"), FAKE_GH)?;
        let fixture = Self {
            _scratch: scratch,
            workspace,
            assets,
            api,
            bin,
            runner_temp,
            calls,
            mise_calls,
            release_index,
            tag_path,
            release_created,
            release_published,
            manifest_copy,
            manifest_checksum_copy,
            accepted_directory,
            upload_log,
            commit,
            tag,
        };
        Ok(fixture)
    }

    pub(super) fn run(
        &self,
        responses: &[String],
        tag_commit: &str,
        event_commit: &str,
    ) -> Result<Output, Box<dyn Error>> {
        self.run_with_workflow(
            responses,
            tag_commit,
            event_commit,
            "workflow_dispatch",
            "refs/heads/main",
            "tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/heads/main",
        )
    }

    pub(super) fn run_with_workflow(
        &self,
        responses: &[String],
        tag_commit: &str,
        event_commit: &str,
        event_name: &str,
        ref_name: &str,
        workflow_ref: &str,
    ) -> Result<Output, Box<dyn Error>> {
        for (index, response) in responses.iter().enumerate() {
            fs::write(self.api.join(format!("release-{index}.json")), response)?;
        }
        let mut paths = vec![self.bin.clone()];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        let path = env::join_paths(paths)?;
        Ok(Command::new("python3")
            .arg(publisher_path())
            .args(["--version", RELEASE_VERSION])
            .current_dir(&self.workspace)
            .env("PATH", path)
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("GITHUB_REF", ref_name)
            .env("GITHUB_EVENT_NAME", event_name)
            .env("GITHUB_WORKFLOW_REF", workflow_ref)
            .env("GITHUB_WORKFLOW_SHA", event_commit)
            .env("GITHUB_SHA", event_commit)
            .env("GH_TOKEN", "fixture-token")
            .env("GITHUB_TOKEN", "")
            .env("MISE_GITHUB_TOKEN", "")
            .env("GH_FAKE_MAIN_SHA", &self.commit)
            .env("GH_FAKE_TAG_SHA", tag_commit)
            .env("GH_FAKE_API_DIR", &self.api)
            .env("GH_FAKE_RELEASE_INDEX", &self.release_index)
            .env("GH_FAKE_TAG_PATH", &self.tag_path)
            .env("GH_FAKE_RELEASE_CREATED", &self.release_created)
            .env("GH_FAKE_RELEASE_PUBLISHED", &self.release_published)
            .env("GH_FAKE_ACCEPTED_DIRECTORY", &self.accepted_directory)
            .env("GH_FAKE_CALLS", &self.calls)
            .env("GH_FAKE_MISE_CALLS", &self.mise_calls)
            .env("GH_FAKE_MANIFEST_COPY", &self.manifest_copy)
            .env(
                "GH_FAKE_MANIFEST_CHECKSUM_COPY",
                &self.manifest_checksum_copy,
            )
            .env("GH_FAKE_UPLOADS", &self.upload_log)
            .output()?)
    }

    pub(super) fn manifest_expected(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let records = asset_records(RELEASE_VERSION, &self.tag);
        let response = release_json(true, false, &self.tag, &records);
        let release_path = self._scratch.path().join("manifest-input.json");
        fs::write(&release_path, response)?;
        let output = Command::new("python3")
            .arg(helper_path())
            .args([
                "--mode",
                "create",
                "--release-json",
                release_path.to_str().ok_or("release path is not UTF-8")?,
                "--asset-dir",
                self.assets.to_str().ok_or("asset path is not UTF-8")?,
                "--manifest",
                self.assets
                    .join(MANIFEST_NAME)
                    .to_str()
                    .ok_or("manifest path is not UTF-8")?,
                "--version",
                RELEASE_VERSION,
                "--repository",
                REPOSITORY,
                "--commit",
                &self.commit,
                "--tag",
                &self.tag,
                "--tag-commit",
                &self.commit,
            ])
            .output()?;
        assert_success(&output);
        let digest = output_text(&output)?
            .lines()
            .find_map(|line| line.strip_prefix("release_manifest_sha256="))
            .ok_or("missing manifest digest")?;
        let checksum_digest = output_text(&output)?
            .lines()
            .find_map(|line| line.strip_prefix("release_manifest_checksum_sha256="))
            .ok_or("missing manifest checksum digest")?;
        let size = fs::metadata(self.assets.join(MANIFEST_NAME))?.len();
        let checksum_size = fs::metadata(
            self.assets
                .join("velnor-actions-release-manifest.json.sha256"),
        )?
        .len();
        let mut final_records = records;
        final_records.push(asset_record(
            MANIFEST_NAME,
            size,
            digest,
            Some(&asset_url(&self.tag, MANIFEST_NAME)),
        ));
        final_records.push(asset_record(
            "velnor-actions-release-manifest.json.sha256",
            checksum_size,
            checksum_digest,
            Some(&asset_url(
                &self.tag,
                "velnor-actions-release-manifest.json.sha256",
            )),
        ));
        Ok(final_records)
    }
}

fn copy_build_assets(source: &Path, workspace: &Path) -> Result<(), Box<dyn Error>> {
    for (target, directory) in [
        (LINUX_TARGET, "linux-assets"),
        (MACOS_TARGET, "macos-assets"),
    ] {
        let destination = workspace.join(directory);
        fs::create_dir_all(&destination)?;
        let binary = format!("velnor-actions-{RELEASE_VERSION}-{target}");
        for name in [binary.clone(), format!("{binary}.sha256")] {
            fs::copy(source.join(&name), destination.join(&name))?;
        }
    }
    Ok(())
}

fn write_executable(path: &Path, source: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, source)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn helper_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/create-release-manifest.py")
}

fn publisher_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/publish_generator_release.py")
}
