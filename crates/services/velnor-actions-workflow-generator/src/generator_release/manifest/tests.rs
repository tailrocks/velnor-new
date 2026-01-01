use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_PRODUCER: &str =
    include_str!("../../../../../../scripts/generator-release/create-release-manifest.sh");

mod bundle;
mod macos;
mod preflight;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn scratch() -> Result<Scratch, Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "velnor-release-preflight-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory)?;
    Ok(Scratch(directory))
}

fn find_command(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").ok_or("missing PATH")?) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return candidate.canonicalize().map_err(Into::into);
        }
    }
    Err(format!("cannot find test command {name}").into())
}
