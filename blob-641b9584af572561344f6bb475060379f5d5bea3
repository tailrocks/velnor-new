use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use self::stubs::{GH_STUB, GIT_STUB, MISE_STUB, TIMEOUT_STUB};
use super::Family;

#[path = "schema2_product_release_exec_stubs.rs"]
mod stubs;

const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";
const REPOSITORY: &str = "tailrocks/velnor-new";
const WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main";
static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Fixture {
    root: PathBuf,
    bin: PathBuf,
    calls: PathBuf,
    output: PathBuf,
    family: Family,
    mode: &'static str,
}

pub(super) struct ResultRecord {
    pub(super) success: bool,
    pub(super) stderr: String,
    pub(super) output: String,
    pub(super) calls: String,
}

impl Fixture {
    pub(super) fn new(family: Family, mode: &'static str) -> Result<Self, Box<dyn Error>> {
        if family == Family::Generator {
            return Err("generator release requires the canonical manifest fixture".into());
        }
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-product-release-{}-{id}",
            std::process::id()
        ));
        let bin = root.join("bin");
        fs::create_dir_all(&bin)?;
        write_executable(&bin.join("mise"), MISE_STUB)?;
        write_executable(&bin.join("gh"), GH_STUB)?;
        write_executable(&bin.join("timeout"), TIMEOUT_STUB)?;
        write_executable(&bin.join("git"), GIT_STUB)?;
        let fixture = Self {
            calls: root.join("calls"),
            output: root.join("outputs"),
            root,
            bin,
            family,
            mode,
        };
        fs::write(&fixture.calls, "")?;
        fs::write(&fixture.output, "")?;
        fixture.write_api_fixtures()?;
        Ok(fixture)
    }

    pub(super) fn run(
        &self,
        script: String,
        action: Option<&str>,
    ) -> Result<ResultRecord, Box<dyn Error>> {
        self.run_with_failure(script, action, None)
    }

    pub(super) fn run_with_failure(
        &self,
        script: String,
        action: Option<&str>,
        failure: Option<&str>,
    ) -> Result<ResultRecord, Box<dyn Error>> {
        let path = std::env::var("PATH")?;
        let mut command = Command::new("bash");
        command
            .args(["-euo", "pipefail", "-c"])
            .arg(script)
            .current_dir(&self.root)
            .env("PATH", format!("{}:{path}", self.bin.display()))
            .env("VELNOR_TEST_ROOT", &self.root)
            .env("VELNOR_TEST_CALLS", &self.calls)
            .env("VELNOR_TEST_MODE", self.mode)
            .env("VELNOR_TEST_SOURCE", SOURCE)
            .env("VELNOR_TEST_TAG", self.tag())
            .env("VELNOR_TEST_ASSETS", self.assets().join(" "))
            .env("VELNOR_TEST_FAIL", failure.unwrap_or_default())
            .env("VELNOR_RELEASE_CI_POLL_LIMIT", "1")
            .env("VELNOR_RELEASE_CI_POLL_SECONDS", "0")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_EVENT_NAME", "workflow_dispatch")
            .env("GITHUB_WORKFLOW_REF", WORKFLOW_REF)
            .env("GITHUB_SHA", SOURCE)
            .env("GITHUB_WORKFLOW_SHA", SOURCE)
            .env("GH_TOKEN", "fixture-token")
            .env("GITHUB_OUTPUT", &self.output)
            .env("VELNOR_SOURCE_SHA", SOURCE)
            .env("VELNOR_WORKFLOW_AUTHORITY_SHA", SOURCE);
        if let Some(action) = action {
            command
                .env("VELNOR_RELEASE_ACTION", action)
                .env("VELNOR_CI_RUN_ID", "42")
                .env("VELNOR_CI_ATTEMPT", "1");
        }
        let output = command.output()?;
        Ok(ResultRecord {
            success: output.status.success(),
            stderr: String::from_utf8(output.stderr)?,
            output: fs::read_to_string(&self.output)?,
            calls: fs::read_to_string(&self.calls)?,
        })
    }

    fn assets(&self) -> &'static [&'static str] {
        match self.family {
            Family::Images => &[
                "velnor-runner-linux-amd64.tar",
                "velnor-dind-linux-amd64.tar",
                "velnor-resource-probe-linux-amd64.tar",
                "RESOURCE_PROBE_MANIFEST.json",
                "SHA256SUMS",
            ],
            Family::Binary => &["velnor-host", "SHA256SUMS"],
            Family::Generator => &[],
        }
    }

    fn tag(&self) -> String {
        format!("{}-{SOURCE}", self.family.tag_prefix())
    }

    fn write_api_fixtures(&self) -> Result<(), Box<dyn Error>> {
        let initial = match self.mode {
            "draft" => release_page(self, true, false),
            "complete" | "wrong-target" => release_page(self, false, true),
            _ => "[[]]".to_owned(),
        };
        fs::write(self.root.join("releases.json"), initial)?;
        fs::write(
            self.root.join("published.json"),
            release_object(self, false, self.mode != "mutable"),
        )?;
        let ref_page = if matches!(self.mode, "complete" | "draft" | "orphan" | "wrong-target") {
            format!(r#"[[{{"ref":"refs/tags/{}"}}]]"#, self.tag())
        } else {
            "[[]]".to_owned()
        };
        fs::write(self.root.join("refs.json"), ref_page)?;
        let tag_sha = if self.mode == "wrong-target" {
            "fedcba9876543210fedcba9876543210fedcba98"
        } else {
            SOURCE
        };
        fs::write(
            self.root.join("tag.json"),
            format!(r#"{{"object":{{"type":"commit","sha":"{tag_sha}"}}}}"#),
        )?;
        let main_sha = if self.mode == "stale" {
            "fedcba9876543210fedcba9876543210fedcba98"
        } else {
            SOURCE
        };
        fs::write(
            self.root.join("main.json"),
            format!(r#"{{"sha":"{main_sha}"}}"#),
        )?;
        fs::write(
            self.root.join("runs.json"),
            format!(
                r#"[{{"workflow_runs":[{{"id":42,"run_number":12,"run_attempt":1,"head_repository":{{"full_name":"{REPOSITORY}"}},"head_sha":"{SOURCE}","head_branch":"main","event":"push","status":"completed","conclusion":"success","path":".github/workflows/ci.yml"}}]}}]"#
            ),
        )?;
        fs::write(
            self.root.join("runs-changed.json"),
            format!(
                r#"[{{"workflow_runs":[{{"id":42,"run_number":12,"run_attempt":2,"head_repository":{{"full_name":"{REPOSITORY}"}},"head_sha":"{SOURCE}","head_branch":"main","event":"push","status":"completed","conclusion":"success","path":".github/workflows/ci.yml"}}]}}]"#
            ),
        )?;
        fs::write(
            self.root.join("jobs.json"),
            format!(
                r#"[{{"jobs":[{{"id":300,"run_id":42,"run_attempt":1,"head_sha":"{SOURCE}","head_branch":"main","name":"Required","status":"completed","conclusion":"success"}}]}}]"#
            ),
        )?;
        fs::write(self.root.join("tag-created"), "")?;
        if !matches!(self.mode, "complete" | "draft" | "orphan" | "wrong-target") {
            fs::remove_file(self.root.join("tag-created"))?;
        }
        Ok(())
    }

    pub(super) fn create_build_assets(&self) -> Result<(), Box<dyn Error>> {
        match self.family {
            Family::Images | Family::Binary => {
                let directory = self.root.join("assets");
                fs::create_dir_all(&directory)?;
                for asset in self.assets().iter().filter(|asset| **asset != "SHA256SUMS") {
                    fs::write(directory.join(asset), format!("fixture bytes: {asset}"))?;
                }
                let sums = checksum_file(
                    &directory,
                    self.assets()
                        .iter()
                        .copied()
                        .filter(|asset| *asset != "SHA256SUMS"),
                )?;
                fs::write(directory.join("SHA256SUMS"), sums)?;
            }
            Family::Generator => {
                return Err("generator release assets need canonical provenance".into());
            }
        }
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("release test fixture cleanup failed: {error}");
        }
    }
}

fn release_page(fixture: &Fixture, draft: bool, immutable: bool) -> String {
    format!("[[{}]]", release_object(fixture, draft, immutable))
}

fn release_object(fixture: &Fixture, draft: bool, immutable: bool) -> String {
    let assets = fixture
        .assets()
        .iter()
        .map(|asset| {
            format!(
                r#"{{"name":"{asset}","state":"uploaded","size":10,"digest":"sha256:{}"}}"#,
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"tag_name":"{}","target_commitish":"{SOURCE}","draft":{draft},"prerelease":false,"immutable":{immutable},"assets":[{assets}]}}"#,
        fixture.tag()
    )
}

fn checksum_file<'a>(
    directory: &Path,
    assets: impl IntoIterator<Item = &'a str>,
) -> Result<String, Box<dyn Error>> {
    let output = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .args(assets)
        .current_dir(directory)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8(output.stderr)?.into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn write_executable(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(super) fn contains_mutation(calls: &str) -> bool {
    [
        "git/refs",
        "release create",
        "release upload",
        "release edit",
    ]
    .iter()
    .any(|command| calls.contains(command))
}
