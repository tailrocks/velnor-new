use super::scripts;
use crate::schema2::git_fixture;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const SOURCE_SHA_ENV: &str = "EXPECTED_SOURCE_SHA";
const ASSET: &str = "repo-scan-aarch64-apple-darwin";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("velnor-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

struct Workspace {
    scratch: Scratch,
    root: PathBuf,
    runner_temp: PathBuf,
    metadata: PathBuf,
    source_sha: String,
}

impl Workspace {
    fn new(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let root = scratch.path().join("workspace");
        fs::create_dir_all(root.join("crates/repo-scan/src")).expect("create virtual workspace");
        let root = fs::canonicalize(root).expect("canonical workspace root");
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/repo-scan\"]\nresolver = \"2\"\n",
        )
        .expect("write virtual workspace manifest");
        fs::write(
            root.join("crates/repo-scan/Cargo.toml"),
            "[package]\nname = \"repo-scan\"\nversion = \"0.4.3\"\n\n[[bin]]\nname = \"repo-scan\"\npath = \"src/main.rs\"\n",
        )
        .expect("write member manifest");
        fs::write(root.join("crates/repo-scan/src/main.rs"), "fn main() {}\n")
            .expect("write binary source");
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"repo-scan\"\nversion = \"0.4.3\"\n",
        )
        .expect("write lockfile");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        git(&root, &["config", "user.name", "Velnor tests"]);
        git(
            &root,
            &["add", "Cargo.toml", "Cargo.lock", "crates/repo-scan"],
        );
        git(&root, &["commit", "-q", "-m", "fixture"]);
        let source_sha = git_output(&root, &["rev-parse", "HEAD"]);
        let runner_temp = scratch.path().join("runner-temp");
        fs::create_dir(&runner_temp).expect("create runner temp");
        let metadata = scratch.path().join("metadata.json");
        fs::write(&metadata, metadata_json(&root, &[], false)).expect("write metadata");
        Self {
            scratch,
            root,
            runner_temp,
            metadata,
            source_sha,
        }
    }

    fn env(&self, output: &Path) -> Vec<(String, String)> {
        vec![
            (
                "GITHUB_WORKSPACE".to_owned(),
                self.root.display().to_string(),
            ),
            ("MANIFEST_PATH".to_owned(), "Cargo.toml".to_owned()),
            ("PACKAGE_NAME".to_owned(), "repo-scan".to_owned()),
            ("BINARY_NAME".to_owned(), "repo-scan".to_owned()),
            ("GITHUB_OUTPUT".to_owned(), output.display().to_string()),
            (
                "METADATA_FILE".to_owned(),
                self.metadata.display().to_string(),
            ),
        ]
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = git_fixture::command(root)
        .expect("build fixture Git command")
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let output = git_fixture::command(root)
        .expect("build fixture Git command")
        .args(args)
        .output()
        .expect("run git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout)
        .expect("git output UTF-8")
        .trim()
        .to_owned()
}

fn metadata_json(root: &Path, required_features: &[&str], outside: bool) -> String {
    let member_manifest = if outside {
        root.parent()
            .expect("scratch parent")
            .join("outside/Cargo.toml")
    } else {
        root.join("crates/repo-scan/Cargo.toml")
    };
    let features = required_features
        .iter()
        .map(|feature| format!("\"{feature}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"packages":[{{"name":"repo-scan","version":"0.4.3","id":"path+file:///workspace/crates/repo-scan#repo-scan@0.4.3","manifest_path":"{}","targets":[{{"name":"repo-scan","kind":["bin"],"required-features":[{}]}}]}}],"workspace_members":["path+file:///workspace/crates/repo-scan#repo-scan@0.4.3"],"workspace_root":"{}"}}"#,
        member_manifest.display(),
        features,
        root.display()
    )
}

fn run_script(script: &str, env: &[(String, String)], path: Option<&Path>) -> Output {
    let scratch = Scratch::new("script");
    let script_path = scratch.path().join("run.sh");
    fs::write(&script_path, script).expect("write shell script");
    let mut command = Command::new("bash");
    command.arg(&script_path);
    if let Some((_, workspace)) = env.iter().find(|(key, _)| key == "GITHUB_WORKSPACE") {
        command.current_dir(workspace);
    }
    command.envs(env.iter().map(|(key, value)| (key, value)));
    if let Some(path) = path {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        command.env(
            "PATH",
            format!("{}:{}", path.display(), inherited.to_string_lossy()),
        );
    }
    command.output().expect("run shell script")
}

fn successful(output: &Output) -> bool {
    output.status.success()
}

#[path = "schema2_consumer_binary_release_identity_build_tests.rs"]
mod identity_build_tests;

#[path = "schema2_consumer_binary_release_publisher_tests.rs"]
mod publisher_tests;
