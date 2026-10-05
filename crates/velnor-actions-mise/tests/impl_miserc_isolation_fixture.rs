use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const STAGE_MARKER: &str = ".velnor-miserc-stage-owner";
const BAD_MISERC: &str = "invalid = [\n";
static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Stage {
    base: PathBuf,
    pub(super) root: PathBuf,
    pub(super) nested: PathBuf,
    pub(super) home: PathBuf,
    pub(super) xdg: PathBuf,
    pub(super) global: PathBuf,
    pub(super) system: PathBuf,
    pub(super) bin: PathBuf,
    pub(super) marker: String,
    owned_base: Option<PathBuf>,
}

impl Stage {
    pub(super) fn new() -> Result<Self, String> {
        let temp_root = std::env::temp_dir()
            .canonicalize()
            .map_err(|err| format!("cannot resolve temp directory: {err}"))?;
        if temp_root == Path::new("/") || !temp_root.is_dir() {
            return Err(format!("unsafe temp directory: {}", temp_root.display()));
        }
        for _ in 0..128 {
            let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let base = temp_root.join(format!("velnor-miserc-{}-{id}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&base) {
                Ok(()) => {
                    let marker = format!("{}-{id}", std::process::id());
                    let owned_base = base.clone();
                    let stage = Self::from_parts(&base, marker, Some(owned_base));
                    stage.prepare()?;
                    return Ok(stage);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("cannot create mise fixture root: {error}")),
            }
        }
        Err("cannot reserve a unique mise fixture root".to_owned())
    }

    pub(super) fn from_child_base(base: &Path, marker: String) -> Result<Self, String> {
        let temp_root = std::env::temp_dir()
            .canonicalize()
            .map_err(|err| format!("cannot resolve temp directory: {err}"))?;
        let canonical = base
            .canonicalize()
            .map_err(|err| format!("cannot resolve child fixture root: {err}"))?;
        if canonical != base || canonical.parent() != Some(temp_root.as_path()) {
            return Err(format!(
                "child fixture root is outside temp root: {}",
                base.display()
            ));
        }
        let metadata = fs::symlink_metadata(&canonical).map_err(|err| err.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("child fixture root must be a real directory".to_owned());
        }
        let stage = Self::from_parts(&canonical, marker, None);
        stage.validate_contents()?;
        Ok(stage)
    }

    fn from_parts(base: &Path, marker: String, owned_base: Option<PathBuf>) -> Self {
        Self {
            base: base.to_path_buf(),
            root: base.join("project"),
            nested: base.join("project/nested"),
            home: base.join("home"),
            xdg: base.join("home/.config"),
            global: base.join("home/.config/mise"),
            system: base.join("system-mise"),
            bin: base.join("bin"),
            marker,
            owned_base,
        }
    }

    fn prepare(&self) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.base.join(STAGE_MARKER))
            .map_err(|err| err.to_string())?;
        file.write_all(self.marker.as_bytes())
            .map_err(|err| err.to_string())?;
        for path in [
            &self.root,
            &self.nested,
            &self.home,
            &self.xdg,
            &self.global,
            &self.system,
            &self.bin,
        ] {
            fs::create_dir(path).map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    fn validate_contents(&self) -> Result<(), String> {
        let marker = self.base.join(STAGE_MARKER);
        let metadata = fs::symlink_metadata(&marker).map_err(|err| err.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("child fixture marker must be a regular file".to_owned());
        }
        let content = fs::read_to_string(marker).map_err(|err| err.to_string())?;
        if content != self.marker {
            return Err("child fixture marker does not match".to_owned());
        }
        for path in [
            &self.root,
            &self.nested,
            &self.home,
            &self.xdg,
            &self.global,
            &self.system,
            &self.bin,
        ] {
            let metadata = fs::symlink_metadata(path).map_err(|err| err.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!("unsafe child fixture path: {}", path.display()));
            }
        }
        Ok(())
    }

    pub(super) fn write_bad_layers(&self, layers: &[Layer]) -> Result<(), String> {
        for layer in layers {
            let path = match layer {
                Layer::Project => self.root.join(".miserc.toml"),
                Layer::Global => self.global.join("miserc.toml"),
                Layer::System => self.system.join("miserc.toml"),
            };
            fs::write(path, BAD_MISERC).map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    pub(super) fn env(&self, command: &mut Command) {
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.xdg)
            .env("MISE_SYSTEM_CONFIG_DIR", &self.system);
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Some(base) = &self.owned_base {
            let parent = base.parent().and_then(|path| path.canonicalize().ok());
            let root = std::env::temp_dir().canonicalize().ok();
            let metadata = fs::symlink_metadata(base).ok();
            let marker = base.join(STAGE_MARKER);
            let marker_metadata = fs::symlink_metadata(&marker).ok();
            let marker_content = fs::read_to_string(marker).ok();
            if parent == root
                && metadata.is_some_and(|value| value.is_dir() && !value.file_type().is_symlink())
                && marker_metadata
                    .is_some_and(|value| value.is_file() && !value.file_type().is_symlink())
                && marker_content.as_deref() == Some(self.marker.as_str())
                && let Err(error) = fs::remove_dir_all(base)
            {
                eprintln!(
                    "cannot remove owned mise fixture {}: {error}",
                    base.display()
                );
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Layer {
    Project,
    Global,
    System,
}
