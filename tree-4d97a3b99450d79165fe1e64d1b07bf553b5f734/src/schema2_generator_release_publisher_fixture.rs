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
#[path = "schema2_generator_release_fake_commands.rs"]
mod fake_commands;
use self::fake_commands::{FAKE_GH, FAKE_MISE};

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
        let home = scratch.path().join("home");
        let xdg = scratch.path().join("xdg");
        let templates = scratch.path().join("git-templates");
        let hooks = scratch.path().join("git-hooks");
        for directory in [
            &workspace,
            &assets,
            &api,
            &bin,
            &runner_temp,
            &home,
            &xdg,
            &templates,
            &hooks,
        ] {
            fs::create_dir_all(directory)?;
        }
        let python = find_executable("python3")?;
        let git = find_executable("git")?;
        std::os::unix::fs::symlink(&python, bin.join("python3"))?;
        std::os::unix::fs::symlink(&git, bin.join("git"))?;
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
        let output = isolated_command(&git, &bin, &home, &xdg, &templates)
            .args(["init", "-b", "main"])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        fs::write(workspace.join("source.txt"), b"fixture source\n")?;
        let output = isolated_command(&git, &bin, &home, &xdg, &templates)
            .args(["add", "source.txt"])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        let hooks_path = format!("core.hooksPath={}", hooks.display());
        let output = isolated_command(&git, &bin, &home, &xdg, &templates)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                &hooks_path,
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-m",
                "fixture source",
            ])
            .current_dir(&workspace)
            .output()?;
        assert_success(&output);
        let output = isolated_command(&git, &bin, &home, &xdg, &templates)
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
        let home = self._scratch.path().join("home");
        let xdg = self._scratch.path().join("xdg");
        let templates = self._scratch.path().join("git-templates");
        Ok(isolated_command(
            &self.bin.join("python3"),
            &self.bin,
            &home,
            &xdg,
            &templates,
        )
        .arg(publisher_path())
        .args(["--version", RELEASE_VERSION])
        .current_dir(&self.workspace)
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
        let home = self._scratch.path().join("home");
        let xdg = self._scratch.path().join("xdg");
        let templates = self._scratch.path().join("git-templates");
        let output = isolated_command(
            &self.bin.join("python3"),
            &self.bin,
            &home,
            &xdg,
            &templates,
        )
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
        let helper_output = output_text(&output)?;
        let digest = helper_output
            .lines()
            .find_map(|line| line.strip_prefix("release_manifest_sha256="))
            .ok_or("missing manifest digest")?
            .to_owned();
        let checksum_digest = helper_output
            .lines()
            .find_map(|line| line.strip_prefix("release_manifest_checksum_sha256="))
            .ok_or("missing manifest checksum digest")?
            .to_owned();
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
            &digest,
            Some(&asset_url(&self.tag, MANIFEST_NAME)),
        ));
        final_records.push(asset_record(
            "velnor-actions-release-manifest.json.sha256",
            checksum_size,
            &checksum_digest,
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

fn find_executable(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = env::var_os("PATH").ok_or("fixture PATH is unavailable")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("fixture executable not found: {name}").into())
}

fn isolated_command(
    program: &Path,
    bin: &Path,
    home: &Path,
    xdg: &Path,
    templates: &Path,
) -> Command {
    let mut command = Command::new(program);
    let path = env::join_paths([bin]).expect("isolated fixture PATH");
    command
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", xdg)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TEMPLATE_DIR", templates)
        .env("GIT_TERMINAL_PROMPT", "0");
    command
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
