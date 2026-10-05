#![cfg(target_os = "linux")]

use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, Permissions},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{shell, steps};

struct ModeZeroFixture {
    base: PathBuf,
    workspace: PathBuf,
    runner_temp: PathBuf,
    job_root: PathBuf,
    primary: PathBuf,
    cold: PathBuf,
    bundle: PathBuf,
    transport_parent: PathBuf,
    output: PathBuf,
    github_env: PathBuf,
    uid: String,
    gid: String,
    root_runner: bool,
}

#[test]
fn mode_zero_restored_bundle_uses_the_reserved_cold_store_as_non_root() -> Result<(), Box<dyn Error>>
{
    let fixture = ModeZeroFixture::new()?;
    let outcome = fixture.run_import();
    let restore_permissions = fixture.restore_bundle_permissions();
    let remove_fixture = fs::remove_dir_all(&fixture.base);
    outcome?;
    restore_permissions?;
    remove_fixture?;
    Ok(())
}

impl ModeZeroFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let base = PathBuf::from("/var/tmp").join(format!(
            "velnor-mbx-mode-zero-{}-{nonce}",
            std::process::id()
        ));
        private_directory(&base)?;
        let workspace = base.join("workspace");
        let runner_temp = base.join("runner-temp");
        let runner_velnor = runner_temp.join("velnor");
        let job_root = runner_velnor.join("mbx-job-424242-7654321-2-mbx_qualification");
        let primary = job_root.join("store");
        let cold = job_root.join("store-cold");
        let import = job_root.join("import");
        let transport_parent = workspace.join(".velnor-mbx-cache");
        let lease = workspace.join(".velnor-mbx-cache.lock");
        let bundle = transport_parent.join("mbx-single-bundle");
        let nested = bundle.join("cas/v1/blake3");
        for directory in [
            &workspace,
            &runner_temp,
            &runner_velnor,
            &job_root,
            &primary,
            &cold,
            &import,
            &transport_parent,
            &lease,
            &nested,
        ] {
            private_directory(directory)?;
        }
        fs::write(nested.join("object"), b"partial restored bundle")?;
        fs::set_permissions(&nested, Permissions::from_mode(0o000))?;
        fs::set_permissions(&bundle, Permissions::from_mode(0o000))?;
        let output = base.join("github-output");
        let github_env = base.join("github-env");
        fs::write(&output, b"")?;
        fs::write(&github_env, b"")?;
        let current_uid = command_text("id", &["-u"])?;
        let root_runner = current_uid == "0";
        let (uid, gid) = if root_runner {
            (
                command_text("id", &["-u", "nobody"])?,
                command_text("id", &["-g", "nobody"])?,
            )
        } else {
            (current_uid, command_text("id", &["-g"])?)
        };
        if root_runner {
            let owner = format!("{uid}:{gid}");
            checked_command(Command::new("chown").arg("-R").arg(owner).arg(&base))?;
        }
        Ok(Self {
            base,
            workspace,
            runner_temp,
            job_root,
            primary,
            cold,
            bundle,
            transport_parent,
            output,
            github_env,
            uid,
            gid,
            root_runner,
        })
    }

    fn environment(&self) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        let mut values = BTreeMap::from([
            ("RUNNER_OS".to_owned(), "Linux".to_owned()),
            ("RUNNER_TEMP".to_owned(), text(&self.runner_temp)),
            ("GITHUB_WORKSPACE".to_owned(), text(&self.workspace)),
            ("GITHUB_OUTPUT".to_owned(), text(&self.output)),
            ("GITHUB_ENV".to_owned(), text(&self.github_env)),
            ("GITHUB_REPOSITORY_ID".to_owned(), "424242".to_owned()),
            ("GITHUB_RUN_ID".to_owned(), "7654321".to_owned()),
            ("GITHUB_RUN_ATTEMPT".to_owned(), "2".to_owned()),
            ("GITHUB_JOB".to_owned(), "mbx_qualification".to_owned()),
            (
                "VELNOR_MBX_JOB_IDENTITY".to_owned(),
                "424242:7654321:2:mbx_qualification".to_owned(),
            ),
            (
                "MBX_CACHE_EXPORT_GROUP".to_owned(),
                "velnor-424242-7654321-2-mbx_qualification".to_owned(),
            ),
            ("MBX_CACHE_RESTORE_OUTCOME".to_owned(), "success".to_owned()),
            (
                "MBX_CACHE_RESTORE_MATCHED_KEY".to_owned(),
                "qualification-writer-key".to_owned(),
            ),
            ("MBX_CACHE_DIR".to_owned(), text(&self.primary)),
        ]);
        for (path_key, identity_key, path) in [
            (
                "VELNOR_MBX_JOB_ROOT",
                "VELNOR_MBX_JOB_ROOT_IDENTITY",
                &self.job_root,
            ),
            (
                "VELNOR_MBX_WORKSPACE_ROOT",
                "VELNOR_MBX_WORKSPACE_IDENTITY",
                &self.workspace,
            ),
            (
                "VELNOR_MBX_TRANSPORT_PARENT",
                "VELNOR_MBX_TRANSPORT_PARENT_IDENTITY",
                &self.transport_parent,
            ),
        ] {
            values.insert(path_key.to_owned(), text(path));
            values.insert(identity_key.to_owned(), identity(path)?);
        }
        let import = self.job_root.join("import");
        let lease = self.workspace.join(".velnor-mbx-cache.lock");
        for (path_key, identity_key, path) in [
            (
                "VELNOR_MBX_PRIMARY_CACHE_DIR",
                "VELNOR_MBX_PRIMARY_IDENTITY",
                &self.primary,
            ),
            (
                "VELNOR_MBX_COLD_CACHE_DIR",
                "VELNOR_MBX_COLD_IDENTITY",
                &self.cold,
            ),
            (
                "VELNOR_MBX_IMPORT_CWD",
                "VELNOR_MBX_IMPORT_IDENTITY",
                &import,
            ),
            (
                "VELNOR_MBX_TRANSPORT_LEASE",
                "VELNOR_MBX_TRANSPORT_LEASE_IDENTITY",
                &lease,
            ),
            (
                "VELNOR_MBX_BUNDLE_PATH",
                "VELNOR_MBX_BUNDLE_IDENTITY",
                &self.bundle,
            ),
        ] {
            values.insert(path_key.to_owned(), text(path));
            values.insert(identity_key.to_owned(), identity(path)?);
        }
        Ok(values)
    }

    fn run_import(&self) -> Result<(), Box<dyn Error>> {
        if self.root_runner {
            let output = Command::new("setpriv")
                .arg("--reuid")
                .arg(&self.uid)
                .arg("--regid")
                .arg(&self.gid)
                .arg("--clear-groups")
                .args(["id", "-u"])
                .output()?;
            if !output.status.success() || String::from_utf8(output.stdout)?.trim() != self.uid {
                return Err(io::Error::other("fixture did not enter its non-root uid").into());
            }
        }
        let lifecycle = steps()?;
        let (script, _) = shell(&lifecycle.restore[1])
            .ok_or_else(|| io::Error::other("import step is not a shell"))?;
        let mut command = if self.root_runner {
            Command::new("setpriv")
        } else {
            Command::new("bash")
        };
        if self.root_runner {
            command
                .arg("--reuid")
                .arg(&self.uid)
                .arg("--regid")
                .arg(&self.gid)
                .arg("--clear-groups")
                .arg("bash");
        }
        command
            .arg("-c")
            .arg(script)
            .env_clear()
            .env("PATH", std::env::var("PATH")?)
            .env("HOME", &self.base);
        for (key, value) in self.environment()? {
            command.env(key, value);
        }
        let output = command.output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "import returned {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        self.assert_cold_fallback()
    }

    fn assert_cold_fallback(&self) -> Result<(), Box<dyn Error>> {
        if self.bundle.exists() || !directory_is_empty(&self.primary)? {
            return Err(io::Error::other("incompatible restore was not cleared").into());
        }
        let outputs = fs::read_to_string(&self.output)?;
        let selected_cold = format!("cache_dir={}\n", text(&self.cold));
        if !outputs.contains("store_ready=true\nexport_allowed=false\n")
            || !outputs.contains(&selected_cold)
            || !fs::read_to_string(&self.github_env)?
                .contains(&format!("MBX_CACHE_DIR={}\n", text(&self.cold)))
        {
            return Err(io::Error::other("import did not select a non-saving cold store").into());
        }
        Ok(())
    }

    fn restore_bundle_permissions(&self) -> io::Result<()> {
        if self.bundle.exists() {
            fs::set_permissions(&self.bundle, Permissions::from_mode(0o700))?;
            fs::set_permissions(
                self.bundle.join("cas/v1/blake3"),
                Permissions::from_mode(0o700),
            )?;
        }
        Ok(())
    }
}

fn private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o700))
}

fn identity(path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("stat")
        .args(["-c", "%d:%i", "--"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("cannot stat cleanup fixture path").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn command_text(program: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("{program} command failed")).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn checked_command(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&output.stderr).into_owned()).into());
    }
    Ok(())
}

fn directory_is_empty(path: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(path)?.next().is_none())
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
