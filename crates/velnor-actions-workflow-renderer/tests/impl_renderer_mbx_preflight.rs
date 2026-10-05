#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::StepKind;

use super::impl_renderer_fixtures::{TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, mbx_tool_steps};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "velnor mbx preflight {} {nonce}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("failed to remove test root {}: {error}", self.0.display()),
        }
    }
}

fn write_tool(path: &Path, content: &str, executable: bool) -> std::io::Result<()> {
    fs::write(path, content)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(if executable { 0o755 } else { 0o644 });
    fs::set_permissions(path, permissions)
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

struct Fixture {
    root: TempRoot,
    workspace: PathBuf,
    mbx_root: PathBuf,
    rust_root: PathBuf,
    mbx_bin: PathBuf,
    rustc_bin: PathBuf,
    github_path: PathBuf,
    github_output: PathBuf,
    github_env: PathBuf,
    path: String,
}

impl Fixture {
    fn new() -> std::io::Result<Self> {
        let root = TempRoot::new()?;
        let workspace = root.0.join("checkout workspace");
        fs::create_dir(&workspace)?;
        let fake_bin = root.0.join("fake command bin");
        let mbx_root = root.0.join("mise installs/mr boxington/1.21.1");
        let rust_root = root.0.join("mise installs/rust tools/1.98.1");
        fs::create_dir_all(&fake_bin)?;
        fs::create_dir_all(&mbx_root)?;
        fs::create_dir_all(&rust_root)?;
        write_tool(
            &fake_bin.join("mise"),
            "#!/bin/sh\nset -eu\n[ \"$4\" = where ] || exit 2\nif [ \"$MISE_MODE\" = relative-mbx ] && [ \"$5\" = 'mr-boxington@1.21.1' ]; then printf '%s\\n' relative/mbx; exit 0; fi\nif [ \"$MISE_MODE\" = relative-rust ] && [ \"$5\" = 'rust@1.98.1' ]; then printf '%s\\n' relative/rust; exit 0; fi\ncase \"$5\" in\n  'mr-boxington@1.21.1') printf '%s\\n' \"$MBX_ROOT\" ;;\n  'rust@1.98.1') printf '%s\\n' \"$RUST_ROOT\" ;;\n  *) exit 2 ;;\nesac\n",
            true,
        )?;
        let system_path = std::env::var_os("PATH").unwrap_or_default();
        let path = format!("{}:{}", fake_bin.display(), system_path.to_string_lossy());
        let mbx_bin = mbx_root.join("mbx");
        let rustc_bin = rust_root.join("rustc");
        let github_path = root.0.join("GitHub path file");
        let github_output = root.0.join("GitHub output file");
        let github_env = root.0.join("GitHub env file");
        fs::write(&github_output, "")?;
        fs::write(&github_env, "")?;
        let fixture = Self {
            root,
            workspace,
            mbx_root,
            rust_root,
            mbx_bin,
            rustc_bin,
            github_path,
            github_output,
            github_env,
            path,
        };
        fixture.write_mbx(Some(TEST_MBX_VERSION), true)?;
        fixture.write_rustc(true, true)?;
        Ok(fixture)
    }

    fn write_mbx(&self, version: Option<&str>, executable: bool) -> std::io::Result<()> {
        if version.is_some() {
            write_tool(
                &self.mbx_bin,
                "#!/bin/sh\n[ \"$1\" = --version ] || exit 2\nprintf 'mbx %s\\n' \"$MBX_MODE\"\n",
                executable,
            )
        } else {
            remove_if_present(&self.mbx_bin)
        }
    }

    fn write_rustc(&self, present: bool, executable: bool) -> std::io::Result<()> {
        if present {
            write_tool(
                &self.rustc_bin,
                "#!/bin/sh\n[ \"$1\" = '+1.98.1' ] || exit 2\n[ \"$2\" = -vV ] || exit 2\ncase \"$RUST_MODE\" in\n  missing-toolchain) exit 3 ;;\n  wrong-toolchain) printf 'release: 1.98.0\\n' ;;\n  *) printf 'release: 1.98.1\\n' ;;\nesac\n",
                executable,
            )
        } else {
            remove_if_present(&self.rustc_bin)
        }
    }

    fn run(&self, script: &str, case: ProbeCase) -> std::io::Result<(bool, String, String)> {
        self.reset_workspace()?;
        self.run_at(script, case, &self.workspace)
    }

    fn run_at(
        &self,
        script: &str,
        case: ProbeCase,
        workspace: &Path,
    ) -> std::io::Result<(bool, String, String)> {
        self.write_mbx(case.mbx_version, case.mbx_executable)?;
        self.write_rustc(case.rustc_present, case.rustc_executable)?;
        fs::write(&self.github_path, "")?;
        fs::write(&self.github_output, "")?;
        fs::write(&self.github_env, "")?;
        let result = Command::new("sh")
            .args(["-c", script])
            .env_clear()
            .env("PATH", &self.path)
            .env("RUNNER_TEMP", &self.root.0)
            .env("GITHUB_WORKSPACE", workspace)
            .env(
                "RUNNER_OS",
                if cfg!(target_os = "macos") {
                    "macOS"
                } else {
                    "Linux"
                },
            )
            .env("GITHUB_PATH", &self.github_path)
            .env("GITHUB_OUTPUT", &self.github_output)
            .env("GITHUB_ENV", &self.github_env)
            .env("GITHUB_REPOSITORY_ID", "741852963")
            .env("GITHUB_RUN_ID", "12001")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_JOB", "mbx_lifecycle")
            .env("MBX_ROOT", &self.mbx_root)
            .env("RUST_ROOT", &self.rust_root)
            .env("MBX_MODE", case.mbx_version.unwrap_or("missing"))
            .env("RUST_MODE", case.rust_mode)
            .env("MISE_MODE", case.mise_mode)
            .env("MISE_RUSTUP_HOME", self.root.0.join("rustup home"))
            .env("MISE_CARGO_HOME", self.root.0.join("cargo home"))
            .env("RUSTUP_HOME", self.root.0.join("rustup home"))
            .env("CARGO_HOME", self.root.0.join("cargo home"))
            .output()?;
        let paths = fs::read_to_string(&self.github_path)?;
        let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
        Ok((result.status.success(), paths, stderr))
    }

    fn reset_workspace(&self) -> std::io::Result<()> {
        remove_dir_all_if_present(&self.root.0.join("velnor"))?;
        remove_dir_all_if_present(&self.workspace.join(".velnor-mbx-cache"))?;
        match fs::remove_dir(self.workspace.join(".velnor-mbx-cache.lock")) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn visible_tools(&self, entries: &str) -> std::io::Result<Vec<String>> {
        let new_entries = entries.lines().collect::<Vec<_>>().join(":");
        let path = format!("{new_entries}:{}", self.path);
        let output = Command::new("sh")
            .args(["-c", "command -v mbx; command -v rustc"])
            .env_clear()
            .env("PATH", path)
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("action PATH search failed"));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }
}

fn remove_dir_all_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy)]
struct ProbeCase {
    name: &'static str,
    mbx_version: Option<&'static str>,
    mbx_executable: bool,
    mise_mode: &'static str,
    rust_mode: &'static str,
    rustc_present: bool,
    rustc_executable: bool,
}

fn assert_rejected(fixture: &Fixture, script: &str, case: ProbeCase) -> std::io::Result<()> {
    let (success, paths, stderr) = fixture.run(script, case)?;
    assert!(
        !success,
        "{} must fail before the action: {stderr}",
        case.name
    );
    assert!(paths.is_empty(), "{} must not update PATH", case.name);
    assert!(fs::read_to_string(&fixture.github_output)?.is_empty());
    assert!(fs::read_to_string(&fixture.github_env)?.is_empty());
    assert!(
        !fixture
            .root
            .0
            .join("velnor/mbx-job-741852963-12001-1-mbx_lifecycle")
            .exists(),
        "{} must release its failed exclusive root",
        case.name
    );
    assert!(!fixture.workspace.join(".velnor-mbx-cache.lock").exists());
    assert!(!fixture.workspace.join(".velnor-mbx-cache").exists());
    Ok(())
}

#[path = "impl_renderer_mbx_preflight_cases.rs"]
mod cases;
#[path = "impl_renderer_mbx_preflight_security.rs"]
mod security;
