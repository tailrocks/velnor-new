use super::tool_identity::{
    mise_data_root, official_binary, official_mise_binary, sha256, write_project,
};
use super::*;
use std::os::unix::fs::MetadataExt;

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    binary: PathBuf,
    mise: PathBuf,
    rustup: OsString,
    inherited_path: OsString,
    mise_data: PathBuf,
}

pub(super) struct Stage {
    pub(super) root: PathBuf,
    pub(super) temp: PathBuf,
    pub(super) runner_root: PathBuf,
    pub(super) job_root: PathBuf,
    pub(super) primary: PathBuf,
    pub(super) cold: PathBuf,
    pub(super) import_cwd: PathBuf,
    pub(super) bundle: PathBuf,
    pub(super) project: PathBuf,
    pub(super) nested: PathBuf,
    pub(super) env_file: PathBuf,
    pub(super) github_path: PathBuf,
    pub(super) preflight_outputs: BTreeMap<String, String>,
    pub(super) group: String,
    pub(super) binary: PathBuf,
    rustup: OsString,
    mise_data: PathBuf,
    path: OsString,
    run_id: String,
}

#[derive(Debug)]
pub(super) struct StepResult {
    pub(super) success: bool,
    pub(super) text: String,
    pub(super) outputs: BTreeMap<String, String>,
}

pub(super) struct Selection {
    pub(super) store: PathBuf,
    pub(super) group: String,
}

impl Fixture {
    pub(super) fn new() -> Result<Self, String> {
        let binary = official_binary()?;
        let mise = official_mise_binary()?;
        let rustup =
            std::env::var_os("RUSTUP_HOME").ok_or("RUSTUP_HOME must select Rust 1.98.1")?;
        let inherited_path = std::env::var_os("PATH").ok_or("PATH is unset")?;
        let mise_data = mise_data_root()?;
        Ok(Self {
            root: new_root()?,
            binary,
            mise,
            rustup,
            inherited_path,
            mise_data,
        })
    }

    pub(super) fn stage(
        &self,
        name: &str,
        run_id: &str,
        linked_store: bool,
    ) -> Result<Stage, String> {
        let mut stage = Stage::new(self, name, run_id)?;
        stage.preflight()?;
        if linked_store {
            stage.link_primary_store()?;
        }
        Ok(stage)
    }

    pub(super) fn raw_stage(&self, name: &str, run_id: &str) -> Result<Stage, String> {
        Stage::new(self, name, run_id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.root) {
            eprintln!("fixture cleanup {}: {err}", self.root.display());
        }
    }
}

impl Stage {
    fn new(fixture: &Fixture, name: &str, run_id: &str) -> Result<Self, String> {
        let root = fixture.root.join(run_id);
        let temp = root.join("runner-temp");
        let runner_root = temp.join("velnor");
        let job_root = runner_root.join(format!("mbx-job-{REPOSITORY_ID}-{run_id}-1-{JOB_ID}"));
        let primary = job_root.join("store");
        let cold = job_root.join("store-cold");
        let import_cwd = job_root.join("import");
        let project = fixture.root.join(format!("project-{name}"));
        let nested = project.join("nested/child");
        let bin = root.join("bin");
        for path in [&root, &temp, &project, &nested, &bin] {
            fs::create_dir_all(path).map_err(|err| format!("create {}: {err}", path.display()))?;
        }
        check_mbx_path_lengths(&[
            &root,
            &temp,
            &runner_root,
            &job_root,
            &primary,
            &cold,
            &import_cwd,
            &project,
            &nested,
        ])?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|err| err.to_string())?;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o700))
            .map_err(|err| err.to_string())?;
        for dir in ["home", "config", "cache", "cargo", "mise-cache"] {
            fs::create_dir(root.join(dir)).map_err(|err| err.to_string())?;
        }
        symlink(&fixture.binary, bin.join("mbx")).map_err(|err| err.to_string())?;
        symlink(&fixture.mise, bin.join("mise")).map_err(|err| err.to_string())?;
        let path = std::env::join_paths(
            std::iter::once(bin).chain(std::env::split_paths(&fixture.inherited_path)),
        )
        .map_err(|err| err.to_string())?;
        write_project(&project)?;
        let env_file = root.join("github-env");
        let github_path = root.join("github-path");
        let bundle = project.join(".velnor-mbx-cache/mbx-single-bundle");
        fs::write(&env_file, "").map_err(|err| err.to_string())?;
        fs::write(&github_path, "").map_err(|err| err.to_string())?;
        Ok(Self {
            root,
            temp,
            runner_root,
            job_root,
            primary,
            cold,
            import_cwd,
            bundle,
            project,
            nested,
            env_file,
            github_path,
            preflight_outputs: BTreeMap::new(),
            group: format!("velnor-{REPOSITORY_ID}-{run_id}-1-{JOB_ID}"),
            binary: fixture.binary.clone(),
            rustup: fixture.rustup.clone(),
            mise_data: fixture.mise_data.clone(),
            path,
            run_id: run_id.to_owned(),
        })
    }

    fn preflight(&mut self) -> Result<(), String> {
        let [step, local_setup] =
            super::generated_preflight_steps(&self.rustup, &self.root.join("cargo"))?;
        super::check_lifecycle_shell(&step, "sh")?;
        super::check_local_setup_env(&local_setup, &self.root.join("cargo"))?;
        let label = format!("preflight-{}", self.run_id);
        capture_action_step(
            &local_setup,
            &capture_label(&format!("local-setup-{}", self.run_id)),
        )?;
        let result = super::run_shell(
            &step,
            self,
            &BTreeMap::new(),
            &self.project,
            &self.temp,
            &[],
            &label,
        )?;
        self.check_preflight(&result)?;
        self.preflight_outputs = result.outputs;
        Ok(())
    }

    fn check_preflight(&self, result: &super::StepResult) -> Result<(), String> {
        self.check_preflight_outputs(result)?;
        self.check_preflight_identities(result)?;
        let state = self.check_preflight_environment()?;
        self.check_private_paths()?;
        eprintln!(
            "preflight status=true outputs={:?} GITHUB_ENV={state:?}",
            result.outputs
        );
        Ok(())
    }

    fn check_preflight_outputs(&self, result: &super::StepResult) -> Result<(), String> {
        let expected = [
            "workspace_root",
            "workspace_identity",
            "transport_lease",
            "transport_lease_identity",
            "transport_parent",
            "transport_parent_identity",
            "job_root",
            "job_identity",
            "job_root_identity",
            "cache_dir",
            "cache_dir_identity",
            "cold_cache_dir",
            "cold_cache_dir_identity",
            "bundle_path",
            "bundle_path_identity",
            "export_group",
            "import_cwd",
            "import_cwd_identity",
        ];
        let lease = self.project.join(".velnor-mbx-cache.lock");
        let parent = self.project.join(".velnor-mbx-cache");
        if !result.success
            || result.outputs.len() != expected.len()
            || expected
                .iter()
                .any(|key| !result.outputs.contains_key(*key))
            || result.outputs.get("workspace_root") != Some(&self.project.display().to_string())
            || result.outputs.get("transport_lease") != Some(&lease.display().to_string())
            || result.outputs.get("transport_parent") != Some(&parent.display().to_string())
            || result.outputs.get("job_root") != Some(&self.job_root.display().to_string())
            || result.outputs.get("cache_dir") != Some(&self.primary.display().to_string())
            || result.outputs.get("cold_cache_dir") != Some(&self.cold.display().to_string())
            || result.outputs.get("bundle_path") != Some(&self.bundle.display().to_string())
            || result.outputs.get("export_group") != Some(&self.group)
            || result.outputs.get("import_cwd") != Some(&self.import_cwd.display().to_string())
        {
            return Err(format!(
                "generated preflight failed: {} {:?}",
                result.text, result.outputs
            ));
        }
        Ok(())
    }

    fn check_preflight_identities(&self, result: &super::StepResult) -> Result<(), String> {
        let lease = self.project.join(".velnor-mbx-cache.lock");
        let parent = self.project.join(".velnor-mbx-cache");
        for (key, path) in [
            ("workspace_identity", self.project.as_path()),
            ("transport_lease_identity", lease.as_path()),
            ("transport_parent_identity", parent.as_path()),
            ("job_root_identity", self.job_root.as_path()),
            ("cache_dir_identity", self.primary.as_path()),
            ("cold_cache_dir_identity", self.cold.as_path()),
            ("bundle_path_identity", self.bundle.as_path()),
            ("import_cwd_identity", self.import_cwd.as_path()),
        ] {
            if result.outputs.get(key) != Some(&path_identity(path)?) {
                return Err(format!("preflight identity mismatch for {key}"));
            }
        }
        Ok(())
    }

    fn check_preflight_environment(&self) -> Result<BTreeMap<String, String>, String> {
        let state = super::env_pairs(&self.env_file)?;
        if state.len() != 2
            || state.get("MBX_CACHE_DIR") != Some(&self.primary.display().to_string())
            || state.get("MBX_CACHE_EXPORT_GROUP") != Some(&self.group)
        {
            return Err(format!("preflight GITHUB_ENV mismatch: {state:?}"));
        }
        let paths = fs::read_to_string(&self.github_path).map_err(|err| err.to_string())?;
        let mbx_root = paths
            .lines()
            .nth(1)
            .ok_or("preflight omitted MBX GITHUB_PATH")?;
        let path_mbx = Path::new(mbx_root)
            .join("mbx")
            .canonicalize()
            .map_err(|err| err.to_string())?;
        if sha256(&path_mbx)? != BINARY_SHA256 {
            return Err(format!(
                "preflight MBX digest mismatch: {}",
                path_mbx.display()
            ));
        }
        Ok(state)
    }

    fn check_private_paths(&self) -> Result<(), String> {
        let lease = self.project.join(".velnor-mbx-cache.lock");
        let parent = self.project.join(".velnor-mbx-cache");
        for private in [
            &self.runner_root,
            &self.job_root,
            &self.primary,
            &self.cold,
            &lease,
            &parent,
            &self.bundle,
            &self.import_cwd,
        ] {
            let metadata = fs::symlink_metadata(private).map_err(|err| err.to_string())?;
            if metadata.file_type().is_symlink()
                || private.canonicalize().map_err(|err| err.to_string())? != *private
                || metadata.permissions().mode() & 0o777 != 0o700
            {
                return Err(format!(
                    "preflight path is not private and canonical: {}",
                    private.display()
                ));
            }
        }
        Ok(())
    }

    fn link_primary_store(&self) -> Result<(), String> {
        let external = self.root.join("external-store");
        fs::create_dir(&external).map_err(|err| err.to_string())?;
        fs::remove_dir(&self.primary).map_err(|err| err.to_string())?;
        symlink(external, &self.primary).map_err(|err| err.to_string())
    }

    pub(super) fn command(&self, program: &str, output: &Path, temp: &Path) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("CARGO_HOME", self.root.join("cargo"))
            .env("MISE_DATA_DIR", &self.mise_data)
            .env("MISE_CACHE_DIR", self.root.join("mise-cache"))
            .env("CARGO_BUILD_JOBS", "2")
            .env("CARGO_NET_OFFLINE", "true")
            .env(
                crate::cache_steps::MBX_GC_AUTO_ENV,
                crate::cache_steps::MBX_GC_AUTO_VALUE,
            )
            .env(
                crate::cache_steps::MBX_SHARE_OUT_DIR_ENV,
                crate::cache_steps::MBX_SHARE_OUT_DIR_VALUE,
            )
            .env("MBX_CACHE_DIR", &self.primary)
            .env("MBX_CACHE_EXPORT_GROUP", &self.group)
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_TEMP", temp)
            .env("GITHUB_WORKSPACE", &self.project)
            .env("GITHUB_OUTPUT", output)
            .env("GITHUB_ENV", &self.env_file)
            .env("GITHUB_PATH", &self.github_path)
            .env("GITHUB_REPOSITORY_ID", REPOSITORY_ID)
            .env("GITHUB_RUN_ID", &self.run_id)
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_JOB", JOB_ID)
            .env("MISE_NO_CONFIG", "1")
            .env("MISE_NO_ENV", "1")
            .env("MISE_NO_HOOKS", "1")
            .env("RUSTUP_HOME", &self.rustup)
            .env("RUSTUP_TOOLCHAIN", "1.98.1")
            .env("VELNOR_MBX_REAL", &self.binary)
            .env("PATH", &self.path)
            .stdin(Stdio::null());
        command
    }
}

pub(super) fn path_identity(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path).map_err(|err| err.to_string())?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

fn new_root() -> Result<PathBuf, String> {
    for _ in 0..16 {
        let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("v{}-{id}", std::process::id()));
        match fs::create_dir(&root) {
            Ok(()) => {
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                    .map_err(|err| err.to_string())?;
                return Ok(root);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(format!("create {}: {err}", root.display())),
        }
    }
    Err("cannot allocate private MBX fixture root".to_owned())
}
fn check_mbx_path_lengths(paths: &[&Path]) -> Result<(), String> {
    for candidate in paths {
        let socket_path = candidate.join("cache-agent.sock");
        if socket_path.as_os_str().as_encoded_bytes().len() >= LINUX_SUN_PATH_CAPACITY {
            return Err(format!("MBX path is too long: {}", candidate.display()));
        }
    }
    Ok(())
}
