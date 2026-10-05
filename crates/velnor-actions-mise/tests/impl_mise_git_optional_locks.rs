//! Optional locks suppress status index refreshes while preserving explicit writes.

#[cfg(unix)]
mod unix_tests {
    use crate::impl_mise_git_config::git_fixture;
    use std::ffi::OsString;
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use velnor_actions_mise::GitRequest;

    const CHILD_ROOT: &str = "VELNOR_GIT_LOCKS_CHILD_ROOT";
    const CHILD_TOKEN: &str = "VELNOR_GIT_LOCKS_CHILD_TOKEN";
    const CHILD_REAL_GIT: &str = "VELNOR_GIT_LOCKS_REAL_GIT";
    const CHILD_LOG: &str = "VELNOR_GIT_LOCKS_EFFECTIVE_ENV";
    const CHILD_STAGE_TOKEN: &str = ".velnor-git-locks-owner";
    static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);

    struct Stage {
        base: PathBuf,
        repo: PathBuf,
        home: PathBuf,
        bin: PathBuf,
        temp: PathBuf,
        marker: String,
        owned: bool,
    }

    impl Stage {
        fn new() -> Result<Self, String> {
            let temp = std::env::temp_dir()
                .canonicalize()
                .map_err(|error| format!("cannot resolve temp directory: {error}"))?;
            if temp == Path::new("/") || !temp.is_dir() {
                return Err(format!("unsafe temp directory: {}", temp.display()));
            }
            for _ in 0..128 {
                let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
                let base = temp.join(format!("velnor-git-locks-{}-{id}", std::process::id()));
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                match builder.create(&base) {
                    Ok(()) => {
                        let marker = format!("{}-{id}", std::process::id());
                        let stage = Self::from_parts(&base, marker, true);
                        stage.prepare()?;
                        return Ok(stage);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(format!("cannot reserve Git fixture root: {error}")),
                }
            }
            Err("cannot reserve a unique Git fixture root".to_owned())
        }

        fn from_child(base: &Path, marker: String) -> Result<Self, String> {
            let temp = std::env::temp_dir()
                .canonicalize()
                .map_err(|error| format!("cannot resolve temp directory: {error}"))?;
            let canonical = base
                .canonicalize()
                .map_err(|error| format!("cannot resolve child fixture root: {error}"))?;
            if canonical != base || canonical.parent() != Some(temp.as_path()) {
                return Err(format!(
                    "child fixture root is outside temp directory: {}",
                    base.display()
                ));
            }
            let stage = Self::from_parts(&canonical, marker, false);
            stage.validate()?;
            Ok(stage)
        }

        fn from_parts(base: &Path, marker: String, owned: bool) -> Self {
            Self {
                repo: base.join("repo"),
                home: base.join("home"),
                bin: base.join("bin"),
                temp: base.join("tmp"),
                base: base.to_path_buf(),
                marker,
                owned,
            }
        }

        fn prepare(&self) -> Result<(), String> {
            let mut marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.base.join(CHILD_STAGE_TOKEN))
                .map_err(|error| error.to_string())?;
            marker
                .write_all(self.marker.as_bytes())
                .map_err(|error| error.to_string())?;
            for path in [&self.repo, &self.home, &self.bin, &self.temp] {
                fs::create_dir(path).map_err(|error| error.to_string())?;
            }
            Ok(())
        }

        fn validate(&self) -> Result<(), String> {
            let base_meta = fs::symlink_metadata(&self.base).map_err(|error| error.to_string())?;
            if base_meta.file_type().is_symlink() || !base_meta.is_dir() {
                return Err("fixture root must be a real directory".to_owned());
            }
            let marker_path = self.base.join(CHILD_STAGE_TOKEN);
            let marker_meta =
                fs::symlink_metadata(&marker_path).map_err(|error| error.to_string())?;
            if marker_meta.file_type().is_symlink() || !marker_meta.is_file() {
                return Err("fixture ownership marker must be a regular file".to_owned());
            }
            if fs::read_to_string(marker_path).map_err(|error| error.to_string())? != self.marker {
                return Err("fixture ownership marker does not match".to_owned());
            }
            for path in [&self.repo, &self.home, &self.bin, &self.temp] {
                let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!("unsafe fixture directory: {}", path.display()));
                }
            }
            Ok(())
        }
    }

    impl Drop for Stage {
        fn drop(&mut self) {
            if !self.owned {
                return;
            }
            let temp = std::env::temp_dir().canonicalize().ok();
            let parent = self.base.parent().and_then(|path| path.canonicalize().ok());
            let base_meta = fs::symlink_metadata(&self.base).ok();
            let marker_path = self.base.join(CHILD_STAGE_TOKEN);
            let marker_meta = fs::symlink_metadata(&marker_path).ok();
            let marker = fs::read_to_string(marker_path).ok();
            if parent == temp
                && base_meta
                    .as_ref()
                    .is_some_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
                && marker_meta
                    .as_ref()
                    .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
                && marker.as_deref() == Some(self.marker.as_str())
                && let Err(error) = fs::remove_dir_all(&self.base)
            {
                eprintln!(
                    "cannot remove owned Git fixture {}: {error}",
                    self.base.display()
                );
            }
        }
    }

    fn git(stage: &Stage, repo: &Path, locks: &str, args: &[&str]) -> Result<Output, String> {
        let mut command = git_fixture::command(repo).map_err(|error| error.to_string())?;
        let output = command
            .env("HOME", &stage.home)
            .env("TMPDIR", &stage.temp)
            .env("GIT_OPTIONAL_LOCKS", locks)
            .args(args)
            .output()
            .map_err(|error| format!("cannot run fixture Git: {error}"))?;
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }

    fn make_repo(stage: &Stage) -> Result<(), String> {
        git(
            stage,
            &stage.repo,
            "0",
            &["init", "--quiet", "--initial-branch=main"],
        )?;
        git(stage, &stage.repo, "0", &["config", "user.name", "Test"])?;
        git(
            stage,
            &stage.repo,
            "0",
            &["config", "user.email", "test@example.invalid"],
        )?;
        fs::write(stage.repo.join("file"), "unchanged\n").map_err(|error| error.to_string())?;
        git(stage, &stage.repo, "0", &["add", "--", "file"])?;
        git(
            stage,
            &stage.repo,
            "0",
            &["commit", "--quiet", "-m", "fixture"],
        )?;
        Ok(())
    }

    fn real_git_path() -> Result<PathBuf, String> {
        let path = std::env::var_os("PATH").ok_or("PATH is required to locate Git")?;
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("git");
            if candidate.is_file() {
                return candidate
                    .canonicalize()
                    .map_err(|error| format!("cannot resolve Git executable: {error}"));
            }
        }
        Err("could not locate a Git executable on PATH".to_owned())
    }

    fn write_git_shim(stage: &Stage, real_git: &Path, log: &Path) -> Result<(), String> {
        let script = "#!/bin/sh\nprintf '%s\\n' \"${GIT_OPTIONAL_LOCKS-unset}\" > \"$VELNOR_GIT_LOCKS_EFFECTIVE_ENV\"\nexec \"$VELNOR_GIT_LOCKS_REAL_GIT\" -C \"$VELNOR_GIT_LOCKS_CHILD_REPO\" status --porcelain\n";
        let path = stage.bin.join("git");
        fs::write(path, script).map_err(|error| error.to_string())?;
        fs::set_permissions(stage.bin.join("git"), fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        if !real_git.is_absolute() || !log.is_absolute() {
            return Err("Git shim paths must be absolute".to_owned());
        }
        Ok(())
    }

    fn product_status_child() -> Result<(), String> {
        if std::env::var("GIT_OPTIONAL_LOCKS").as_deref() != Ok("1") {
            return Err("test child was not given hostile GIT_OPTIONAL_LOCKS=1".to_owned());
        }
        let base = PathBuf::from(std::env::var_os(CHILD_ROOT).ok_or("missing child fixture root")?);
        let token = std::env::var(CHILD_TOKEN).map_err(|error| error.to_string())?;
        let stage = Stage::from_child(&base, token)?;
        let command = GitRequest::diff(Vec::new()).command_in(&stage.repo);
        let effective =
            command.spawn_env(&[(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1"))]);
        assert_eq!(
            effective
                .iter()
                .rev()
                .find(|(key, _)| key == "GIT_OPTIONAL_LOCKS"),
            Some(&(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0")))
        );
        let output = command
            .run_bounded(1024 * 1024, Duration::from_secs(30))
            .map_err(|error| error.to_string())?;
        assert!(
            output.success,
            "isolated Git request failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "status unexpectedly printed changes"
        );
        Ok(())
    }

    #[test]
    fn production_git_optional_locks_suppress_status_refresh_under_hostile_parent()
    -> Result<(), String> {
        if std::env::var_os(CHILD_ROOT).is_some() {
            return product_status_child();
        }

        let stage = Stage::new()?;
        make_repo(&stage)?;
        let index = stage.repo.join(".git/index");
        let before = fs::read(&index).map_err(|error| error.to_string())?;
        let file = stage.repo.join("file");
        let modified = fs::metadata(&file)
            .and_then(|metadata| metadata.modified())
            .map_err(|error| error.to_string())?
            + Duration::from_secs(3);
        File::open(&file)
            .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(modified)))
            .map_err(|error| error.to_string())?;

        let real_git = real_git_path()?;
        let log = stage.base.join("effective-env");
        write_git_shim(&stage, &real_git, &log)?;
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let inherited_path = std::env::var_os("PATH").ok_or("PATH is required")?;
        let mut child_path = vec![stage.bin.clone()];
        child_path.extend(std::env::split_paths(&inherited_path));
        let child_path = std::env::join_paths(child_path).map_err(|error| error.to_string())?;
        let temp_root = stage.base.parent().ok_or("stage has no temp root")?;
        let output = Command::new(current)
            .args([
                "--exact",
                "impl_mise_git_optional_locks::unix_tests::production_git_optional_locks_suppress_status_refresh_under_hostile_parent",
                "--nocapture",
            ])
            .env_clear()
            .env(CHILD_ROOT, &stage.base)
            .env(CHILD_TOKEN, &stage.marker)
            .env(CHILD_REAL_GIT, &real_git)
            .env("VELNOR_GIT_LOCKS_CHILD_REPO", &stage.repo)
            .env(CHILD_LOG, &log)
            .env("GIT_OPTIONAL_LOCKS", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("HOME", &stage.home)
            .env("TMPDIR", temp_root)
            .env("PATH", child_path)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("cannot run hostile-parent test child: {error}"))?;
        assert!(
            output.status.success(),
            "product child failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("... ok"),
            "nested harness did not select the product test: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(&log).map_err(|error| error.to_string())?,
            "0\n"
        );
        assert_eq!(fs::read(&index).map_err(|error| error.to_string())?, before);

        assert!(
            git(&stage, &stage.repo, "1", &["status", "--porcelain"])?
                .stdout
                .is_empty()
        );
        assert_ne!(fs::read(&index).map_err(|error| error.to_string())?, before);
        Ok(())
    }

    #[test]
    fn optional_locks_preserve_explicit_add_commit_and_clone_writes() -> Result<(), String> {
        let stage = Stage::new()?;
        make_repo(&stage)?;
        fs::write(stage.repo.join("file"), "changed\n").map_err(|error| error.to_string())?;
        git(&stage, &stage.repo, "0", &["add", "--", "file"])?;
        assert_eq!(
            git(&stage, &stage.repo, "0", &["show", ":file"])?.stdout,
            b"changed\n"
        );
        git(
            &stage,
            &stage.repo,
            "0",
            &["commit", "--quiet", "-m", "changed"],
        )?;
        let clone = stage.base.join("clone");
        let output = git_fixture::sterile_command()
            .env("HOME", &stage.home)
            .env("TMPDIR", &stage.temp)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["clone", "--local", "--no-hardlinks", "--template=", "--"])
            .arg(&stage.repo)
            .arg(&clone)
            .output()
            .map_err(|error| format!("cannot clone fixture repository: {error}"))?;
        assert!(
            output.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(clone.join("file")).map_err(|error| error.to_string())?,
            b"changed\n"
        );
        assert!(clone.join(".git/index").is_file());
        Ok(())
    }
}
