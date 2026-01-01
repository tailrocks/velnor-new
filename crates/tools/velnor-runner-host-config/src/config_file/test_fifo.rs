use std::path::Path;
use std::process::Command;

pub(super) fn create_fifo(directory: &Path, name: &str) -> Result<(), String> {
    let path = directory.join(name);
    let status = Command::new("mkfifo")
        .arg(&path)
        .status()
        .map_err(|error| format!("run mkfifo: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("mkfifo failed for {}", path.display()))
    }
}
