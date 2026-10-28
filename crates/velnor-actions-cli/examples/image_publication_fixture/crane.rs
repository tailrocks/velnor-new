// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    error::Error,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::registry;

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

pub(super) fn run(
    args: &[OsString],
    state: &mut Value,
    state_path: &Path,
) -> Result<ExitCode, Box<dyn Error>> {
    let command = args
        .first()
        .and_then(|arg| arg.to_str())
        .ok_or("missing crane command")?;
    match command {
        "push" if args.len() == 3 => push(args, state, state_path),
        "manifest" if args.len() == 2 => manifest(reference(args)?, state, state_path),
        "blob" if args.len() == 2 => blob(reference(args)?, state, state_path),
        other => Err(format!("unsupported crane command or argument count: {other}").into()),
    }
}

fn reference(args: &[OsString]) -> Result<&str, Box<dyn Error>> {
    args.get(1)
        .and_then(|arg| arg.to_str())
        .ok_or_else(|| "missing crane reference".into())
}

fn push(
    args: &[OsString],
    state: &mut Value,
    state_path: &Path,
) -> Result<ExitCode, Box<dyn Error>> {
    let layout = PathBuf::from(args.get(1).ok_or("missing OCI layout")?);
    let reference = args
        .get(2)
        .and_then(|arg| arg.to_str())
        .ok_or("missing push reference")?;
    if state["crane"]["tag_reference"].as_str() != Some(reference) {
        return Err("unexpected crane push reference".into());
    }
    let (manifest_digest, mut inventory) = registry::image_inventory(&layout)?;
    if !state["crane"]["corrupt_remote_blob"].is_null() {
        registry::corrupt_inventory(
            &mut inventory,
            &manifest_digest,
            &state["crane"]["corrupt_remote_blob"],
        )?;
    }
    let blob_count = if state["crane"]["tag_movement_target"].is_null() {
        None
    } else {
        Some(registry::inventory_blob_count(
            &inventory,
            &manifest_digest,
        )?)
    };
    let (repository, _) = reference.rsplit_once(':').ok_or("missing tag")?;
    state["crane"]["tag_digest"] = json!(manifest_digest);
    state["crane"]["manifest_reference"] = json!(format!("{repository}@{manifest_digest}"));
    state["crane"]["remote"] = json!({"manifest_digest":manifest_digest,"inventory":inventory});
    if let Some(blob_count) = blob_count {
        let mut target = state["crane"]["tag_movement_target"].clone();
        target["remaining_blob_reads"] = json!(blob_count);
        state["crane"]["pending_tag_movement"] = target;
    }
    fs::write(state_path, serde_json::to_vec(state)?)?;
    Ok(ExitCode::SUCCESS)
}

fn manifest(
    reference: &str,
    state: &mut Value,
    state_path: &Path,
) -> Result<ExitCode, Box<dyn Error>> {
    let is_tag = state["crane"]["tag_reference"].as_str() == Some(reference);
    if is_tag {
        wait_at_manifest_lookup_barrier(reference, state, state_path)?;
        if state["crane"]["tag_digest"].is_null()
            && state["crane"]["tag_lookup_output_consumed"] != true
        {
            let outcome = state["crane"]["tag_lookup_output"].clone();
            if !outcome.is_null() {
                state["crane"]["tag_lookup_output_consumed"] = json!(true);
                fs::write(state_path, serde_json::to_vec(state)?)?;
                return replay(&outcome);
            }
        }
        if state["crane"]["tag_digest"].is_null() {
            return Err("tag lookup outcome was not configured".into());
        }
    } else if state["crane"]["manifest_reference"].as_str() != Some(reference) {
        return Err("unexpected crane manifest reference".into());
    }
    let mut bytes = {
        let remote = &state["crane"]["remote"];
        let digest = if is_tag {
            state["crane"]["tag_digest"].as_str()
        } else {
            remote["manifest_digest"].as_str()
        }
        .ok_or("remote manifest is missing")?
        .to_owned();
        let inventory = &remote["inventory"];
        registry::inventory_blob_size(inventory, &digest)?;
        registry::raw_inventory_blob(inventory, &digest)?
    };
    if !is_tag && !state["crane"]["remote_manifest_response_corruption"].is_null() {
        let offset = usize::try_from(
            state["crane"]["remote_manifest_response_corruption"]["offset"]
                .as_u64()
                .ok_or("remote manifest response corruption offset is missing")?,
        )?;
        let byte = bytes
            .get_mut(offset)
            .ok_or("remote manifest response corruption offset is out of range")?;
        *byte ^= 1;
        state["crane"]["remote_manifest_response_corruption"] = Value::Null;
        fs::write(state_path, serde_json::to_vec(state)?)?;
    }
    io::stdout().write_all(&bytes)?;
    Ok(ExitCode::SUCCESS)
}

fn wait_at_manifest_lookup_barrier(
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
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        match fs::symlink_metadata(&release) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => break,
            Ok(_) => return Err("invalid Crane manifest barrier release marker".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err("Crane manifest lookup barrier timed out".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
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

fn blob(reference: &str, state: &mut Value, state_path: &Path) -> Result<ExitCode, Box<dyn Error>> {
    let (repository, digest) = reference
        .split_once('@')
        .ok_or("blob requires one digest-addressed reference")?;
    let expected = state["crane"]["tag_reference"]
        .as_str()
        .and_then(|value| value.rsplit_once(':'))
        .map(|(repository, _)| repository)
        .ok_or("missing repository")?;
    if repository != expected {
        return Err("unexpected crane blob repository".into());
    }
    if state["crane"]["fail_blob_readback"] == true {
        writeln!(io::stderr(), "fixture blob readback failed")?;
        return Ok(ExitCode::from(1));
    }
    let Ok(bytes) = registry::raw_inventory_blob(&state["crane"]["remote"]["inventory"], digest)
    else {
        writeln!(
            io::stderr(),
            "Error: reading blob {reference}: BLOB_UNKNOWN: blob unknown"
        )?;
        return Ok(ExitCode::from(1));
    };
    wait_at_readback_barrier(digest, state, state_path)?;
    io::stdout().write_all(&bytes)?;
    if state["crane"]["corrupt_remote_blob"]["mode"] == "nonzero"
        && registry::is_blob_fault_target(
            &state["crane"]["remote"]["inventory"],
            state["crane"]["remote"]["manifest_digest"]
                .as_str()
                .ok_or("remote manifest is missing")?,
            &state["crane"]["corrupt_remote_blob"],
            digest,
        )?
    {
        io::stdout().flush()?;
        writeln!(
            io::stderr(),
            "fixture returned complete blob bytes with failure status"
        )?;
        return Ok(ExitCode::from(1));
    }
    advance_tag_movement(state, state_path)?;
    Ok(ExitCode::SUCCESS)
}

fn wait_at_readback_barrier(
    digest: &str,
    state: &mut Value,
    state_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let should_block = BlobReadbackBarrier::from_value(&state["crane"]["blob_readback_barrier"])?
        .is_some_and(|barrier| barrier.digest == digest);
    if !should_block {
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
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        match fs::symlink_metadata(&release) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => break,
            Ok(_) => return Err("invalid Crane readback release marker".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err("Crane readback barrier timed out".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
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

fn advance_tag_movement(state: &mut Value, state_path: &Path) -> Result<(), Box<dyn Error>> {
    if state["crane"]["pending_tag_movement"].is_null() {
        return Ok(());
    }
    let remaining = state["crane"]["pending_tag_movement"]["remaining_blob_reads"]
        .as_u64()
        .ok_or("pending tag movement has no remaining read count")?;
    if remaining > 1 {
        state["crane"]["pending_tag_movement"]["remaining_blob_reads"] = json!(remaining - 1);
    } else {
        let movement = state["crane"]["pending_tag_movement"].clone();
        let additions = movement["inventory"]
            .as_object()
            .ok_or("pending tag movement has no inventory")?
            .clone();
        let inventory = state["crane"]["remote"]["inventory"]
            .as_object_mut()
            .ok_or("remote inventory is missing")?;
        inventory.extend(additions);
        state["crane"]["tag_digest"] = movement["digest"].clone();
        state["crane"]["remote"]["manifest_digest"] = movement["digest"].clone();
        state["crane"]["pending_tag_movement"] = Value::Null;
    }
    fs::write(state_path, serde_json::to_vec(state)?)?;
    Ok(())
}

fn replay(outcome: &Value) -> Result<ExitCode, Box<dyn Error>> {
    let code = u8::try_from(outcome["code"].as_u64().ok_or("missing replay exit code")?)?;
    let stdout = outcome["stdout"].as_str().ok_or("missing replay stdout")?;
    let stderr = outcome["stderr"].as_str().ok_or("missing replay stderr")?;
    io::stdout().write_all(stdout.as_bytes())?;
    io::stderr().write_all(stderr.as_bytes())?;
    Ok(ExitCode::from(code))
}
