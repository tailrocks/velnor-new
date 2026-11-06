// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

struct BlobReadbackBarrier {
    digest: String,
}

struct ManifestLookupBarrier {
    reference: String,
}

struct TemporaryMarker(PathBuf);

impl Drop for TemporaryMarker {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_file(&self.0);
    }
}

pub(super) fn wait_at_manifest_lookup_barrier(
    reference: &str,
    state: &mut Value,
    state_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let barrier = ManifestLookupBarrier::from_value(&state["crane"]["manifest_lookup_barrier"])?;
    if barrier.is_none_or(|barrier| barrier.reference != reference) {
        return Ok(());
    }
    state["crane"]["manifest_lookup_barrier"] = Value::Null;
    fs::write(state_path, serde_json::to_vec(state)?)?;
    let home = PathBuf::from(env::var_os("HOME").ok_or("missing fixture home")?);
    let ready = home.join(".image-publication-manifest-ready");
    let release = home.join(".image-publication-manifest-release");
    publish_ready_marker(&ready, reference.as_bytes())?;
    wait_for_release(&release, "Crane manifest lookup barrier timed out")?;
    *state = serde_json::from_slice(&fs::read(state_path)?)?;
    Ok(())
}

fn publish_ready_marker(path: &Path, contents: &[u8]) -> Result<(), Box<dyn Error>> {
    let file_name = path.file_name().ok_or("ready marker has no file name")?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(format!(".tmp-{}", std::process::id()));
    let temporary = path.with_file_name(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let _temporary_marker = TemporaryMarker(temporary.clone());
    let write_result = file.write_all(contents);
    drop(file);
    write_result?;
    fs::hard_link(&temporary, path)?;
    fs::remove_file(&temporary)?;
    Ok(())
}

pub(super) fn wait_at_readback_barrier(
    digest: &str,
    state: &mut Value,
    state_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let barrier = BlobReadbackBarrier::from_value(&state["crane"]["blob_readback_barrier"])?;
    if barrier.is_none_or(|barrier| barrier.digest != digest) {
        return Ok(());
    }
    state["crane"]["blob_readback_barrier"] = Value::Null;
    fs::write(state_path, serde_json::to_vec(state)?)?;
    let home = PathBuf::from(env::var_os("HOME").ok_or("missing fixture home")?);
    let ready = home.join(".image-publication-readback-ready");
    let release = home.join(".image-publication-readback-release");
    let mut ready_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(ready)?;
    ready_file.write_all(digest.as_bytes())?;
    drop(ready_file);
    wait_for_release(&release, "Crane readback barrier timed out")
}

fn wait_for_release(path: &Path, timeout: &'static str) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => break,
            Ok(_) => return Err("invalid Crane barrier release marker".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err(timeout.into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

impl ManifestLookupBarrier {
    fn from_value(value: &Value) -> Result<Option<Self>, Box<dyn Error>> {
        if value.is_null() {
            return Ok(None);
        }
        let fields = value
            .as_object()
            .filter(|fields| fields.len() == 1)
            .ok_or("invalid Crane manifest lookup barrier")?;
        let reference = fields
            .get("reference")
            .and_then(Value::as_str)
            .ok_or("Crane manifest barrier has no reference")?
            .to_owned();
        Ok(Some(Self { reference }))
    }
}

impl BlobReadbackBarrier {
    fn from_value(value: &Value) -> Result<Option<Self>, Box<dyn Error>> {
        if value.is_null() {
            return Ok(None);
        }
        let fields = value
            .as_object()
            .filter(|fields| fields.len() == 1)
            .ok_or("invalid Crane blob readback barrier")?;
        let digest = fields
            .get("digest")
            .and_then(Value::as_str)
            .ok_or("Crane blob readback barrier has no digest")?
            .to_owned();
        Ok(Some(Self { digest }))
    }
}
