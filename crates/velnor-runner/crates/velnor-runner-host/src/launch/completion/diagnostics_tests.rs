use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bollard::models::ContainerInspectResponse;
use rustix::fs::{Mode, OFlags, fstat, openat, symlinkat};

use super::{
    CAPTURE_DEADLINE, CaptureIdentity, DiagnosticRecord, DiagnosticsRoot, MAX_RECORD_BYTES,
    SCOPE_DIRECTORY, diagnostic_record, scope_component,
};

const ID: &str = "a1b2c3d4";
const NAME: &str = "wunit-runner";
const VOLUME: &str = "wunit";

#[test]
fn canaries_from_unapproved_inspect_fields_never_enter_the_record() -> Result<(), String> {
    let info: ContainerInspectResponse = serde_json::from_value(serde_json::json!({
        "Id": ID,
        "Name": format!("/{NAME}"),
        "Image": "sha256:0123456789abcdef",
        "RestartCount": 3,
        "State": {
            "Status": "exited",
            "Running": false,
            "Paused": false,
            "Restarting": false,
            "OOMKilled": false,
            "Dead": false,
            "ExitCode": 1,
            "Error": "secret-state-error",
            "StartedAt": "2026-10-19T00:00:00Z",
            "FinishedAt": "2026-10-19T00:01:00Z",
            "Health": {"Log": [{"Output": "secret-health-output"}]}
        },
        "Config": {
            "Env": ["TOKEN=secret-env-value"],
            "Labels": {
                "velnor.volume": VOLUME,
                "velnor.worker": VOLUME,
                "velnor.role": "runner",
                "secret": "secret-label-value"
            }
        },
        "NetworkSettings": {"Networks": {"secret-network": {"NetworkID": "secret-network-value"}}},
        "UnknownField": "secret-unknown-value"
    }))
    .map_err(|error| error.to_string())?;
    let record = diagnostic_record(info, &capture_identity(ID, VOLUME))
        .map_err(|error| format!("owned inspect record rejected: {error}"))?;
    let text = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    for canary in [
        "secret-state-error",
        "secret-health-output",
        "secret-env-value",
        "secret-label-value",
        "secret-network-value",
        "secret-unknown-value",
    ] {
        if text.contains(canary) {
            return Err(format!("record retained {canary}"));
        }
    }
    Ok(())
}

#[test]
fn rejects_wrong_container_id_or_owner_labels() -> Result<(), String> {
    for body in [
        serde_json::json!({"Id":"other","Name":format!("/{NAME}"),"Config":{"Labels":labels(VOLUME,"runner")}}),
        serde_json::json!({"Id":ID,"Name":format!("/{NAME}"),"Config":{"Labels":labels("foreign","runner")}}),
    ] {
        let info: ContainerInspectResponse =
            serde_json::from_value(body).map_err(|error| error.to_string())?;
        if diagnostic_record(info, &capture_identity(ID, VOLUME)).is_ok() {
            return Err("foreign container passed identity check".to_owned());
        }
    }
    Ok(())
}

#[test]
fn record_size_has_a_closed_bound() -> Result<(), String> {
    let scratch = Scratch::open("record-bound")?;
    let root = DiagnosticsRoot::open(&scratch.path, "repo").map_err(|error| error.to_string())?;
    let directories = root
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let mut oversized = record();
    oversized.state = Some(super::DiagnosticState {
        status: Some("x".repeat(MAX_RECORD_BYTES)),
        running: None,
        paused: None,
        restarting: None,
        oom_killed: None,
        dead: None,
        exit_code: None,
        started_at: None,
        finished_at: None,
    });
    let identity = capture_identity(ID, VOLUME);
    let name = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    if DiagnosticsRoot::write_record(&directories, &identity, &oversized, Instant::now()).is_ok()
        || scratch
            .path
            .join(SCOPE_DIRECTORY)
            .join(scope_component("repo"))
            .join(name)
            .exists()
    {
        return Err("oversized record was written".to_owned());
    }
    Ok(())
}

#[test]
fn expired_capture_deadline_prevents_file_creation() -> Result<(), String> {
    let scratch = Scratch::open("capture-deadline")?;
    let root = DiagnosticsRoot::open(&scratch.path, "repo").map_err(|error| error.to_string())?;
    let directories = root
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let identity = capture_identity(ID, VOLUME);
    let filename = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    let started = Instant::now()
        .checked_sub(CAPTURE_DEADLINE + std::time::Duration::from_millis(1))
        .ok_or_else(|| "capture deadline could not be represented".to_owned())?;
    if DiagnosticsRoot::write_record(&directories, &identity, &record(), started)
        != Err(crate::error::HostError::DockerTimeout)
    {
        return Err("expired capture continued".to_owned());
    }
    let path = scratch
        .path
        .join(SCOPE_DIRECTORY)
        .join(scope_component("repo"))
        .join(filename);
    if path.exists() {
        return Err("expired capture created a file".to_owned());
    }
    Ok(())
}

#[test]
fn private_file_is_create_only_and_symlink_is_rejected() -> Result<(), String> {
    let scratch = Scratch::open("private-file")?;
    let root = DiagnosticsRoot::open(&scratch.path, "repo").map_err(|error| error.to_string())?;
    let directories = root
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let record = record();
    let identity = capture_identity(ID, VOLUME);
    let name = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    DiagnosticsRoot::write_record(&directories, &identity, &record, Instant::now())
        .map_err(|error| error.to_string())?;
    DiagnosticsRoot::write_record(&directories, &identity, &record, Instant::now())
        .map_err(|error| error.to_string())?;
    let path = scratch
        .path
        .join(SCOPE_DIRECTORY)
        .join(scope_component("repo"))
        .join(&name);
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return Err("diagnostic file is not private".to_owned());
    }
    let before = fs::read(&path).map_err(|error| error.to_string())?;
    let mut changed = record.clone();
    changed.image = Some("sha256:changed".to_owned());
    if DiagnosticsRoot::write_record(&directories, &identity, &changed, Instant::now()).is_ok()
        || fs::read(&path).map_err(|error| error.to_string())? != before
    {
        return Err("existing diagnostic output was replaced".to_owned());
    }
    let link_identity = CaptureIdentity {
        intent_id: 2,
        container_id: "b1c2d3e4",
        ..capture_identity(ID, VOLUME)
    };
    let link_name = DiagnosticsRoot::file_name(&link_identity).map_err(|e| e.to_string())?;
    symlinkat(&name, &directories.scope, &link_name).map_err(|error| error.to_string())?;
    let mut link_record = record;
    link_record.intent_id = link_identity.intent_id;
    link_record.container_id = link_identity.container_id.to_owned();
    if DiagnosticsRoot::write_record(&directories, &link_identity, &link_record, Instant::now())
        .is_ok()
    {
        return Err("diagnostic symlink was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn interrupted_or_unknown_output_is_preserved_and_rejected() -> Result<(), String> {
    let scratch = Scratch::open("interrupted-file")?;
    let root = DiagnosticsRoot::open(&scratch.path, "repo").map_err(|error| error.to_string())?;
    let directories = root
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let identity = capture_identity(ID, VOLUME);
    let name = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    let fd = openat(
        &directories.scope,
        &name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|error| error.to_string())?;
    let mut file = std::fs::File::from(fd);
    file.write_all(b"{\"version\":1")
        .map_err(|error| error.to_string())?;
    let path = scratch
        .path
        .join(SCOPE_DIRECTORY)
        .join(scope_component("repo"))
        .join(&name);
    let before = fs::read(&path).map_err(|error| error.to_string())?;
    if DiagnosticsRoot::write_record(&directories, &identity, &record(), Instant::now()).is_ok() {
        return Err("partial output was treated as complete".to_owned());
    }
    if fs::read(&path).map_err(|error| error.to_string())? != before {
        return Err("unknown output was changed".to_owned());
    }
    Ok(())
}

#[test]
fn state_and_output_path_replacement_cannot_redirect_records() -> Result<(), String> {
    let scratch = Scratch::open("root-replaced")?;
    let foreign = Scratch::open("foreign-root")?;
    let root = DiagnosticsRoot::open(&scratch.path, "repo").map_err(|error| error.to_string())?;
    let held_root = scratch.directory.with_extension("held");
    fs::rename(&scratch.directory, &held_root).map_err(|error| error.to_string())?;
    symlink(&foreign.directory, &scratch.directory).map_err(|error| error.to_string())?;

    let directories = root
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let scope_name = scope_component("repo");
    let held_parent = held_root.with_extension("parent-held");
    let parent_path = held_root.join(SCOPE_DIRECTORY);
    fs::rename(&parent_path, &held_parent).map_err(|error| error.to_string())?;
    let foreign_parent = foreign.directory.join("foreign-parent");
    private_dir(&foreign_parent)?;
    symlink(&foreign_parent, &parent_path).map_err(|error| error.to_string())?;

    let scope_path = held_parent.join(&scope_name);
    let held_scope = held_parent.with_extension("scope-held");
    fs::rename(&scope_path, &held_scope).map_err(|error| error.to_string())?;
    let foreign_scope = foreign_parent.join(&scope_name);
    private_dir(&foreign_scope)?;
    symlink(&foreign_scope, &scope_path).map_err(|error| error.to_string())?;

    let identity = capture_identity(ID, VOLUME);
    DiagnosticsRoot::write_record(&directories, &identity, &record(), Instant::now())
        .map_err(|error| error.to_string())?;
    let filename = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    if !held_scope.join(&filename).is_file()
        || foreign_scope.join(&filename).exists()
        || foreign.directory.join(SCOPE_DIRECTORY).exists()
    {
        return Err("replacement path received the diagnostic record".to_owned());
    }

    fs::remove_file(&scope_path).map_err(|error| error.to_string())?;
    fs::remove_dir_all(&held_scope).map_err(|error| error.to_string())?;
    fs::remove_dir_all(&held_parent).map_err(|error| error.to_string())?;
    fs::remove_file(&scratch.directory).map_err(|error| error.to_string())?;
    fs::remove_dir_all(&held_root).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn invalid_connection_path_components_are_rejected() -> Result<(), String> {
    let scratch = Scratch::open("invalid-connection")?;
    for connection_id in ["", ".", ".."] {
        if DiagnosticsRoot::open(&scratch.path, connection_id).is_ok() {
            return Err(format!("accepted invalid connection ID {connection_id:?}"));
        }
    }
    Ok(())
}

#[test]
fn connection_scopes_use_distinct_output_directories() -> Result<(), String> {
    let scratch = Scratch::open("scope")?;
    let first = DiagnosticsRoot::open(&scratch.path, "essential").map_err(|e| e.to_string())?;
    let second = DiagnosticsRoot::open(&scratch.path, "chainargos").map_err(|e| e.to_string())?;
    let first_directory = first
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let second_directory = second
        .output_directories(Instant::now())
        .map_err(|error| error.to_string())?;
    let first_stat = fstat(&first_directory.scope).map_err(|error| error.to_string())?;
    let second_stat = fstat(&second_directory.scope).map_err(|error| error.to_string())?;
    let identity = capture_identity(ID, VOLUME);
    let a = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    let b = DiagnosticsRoot::file_name(&identity).map_err(|e| e.to_string())?;
    if a != b || (first_stat.st_dev, first_stat.st_ino) == (second_stat.st_dev, second_stat.st_ino)
    {
        return Err("connection scopes share diagnostic identity".to_owned());
    }
    Ok(())
}

fn capture_identity<'a>(id: &'a str, volume: &'a str) -> CaptureIdentity<'a> {
    CaptureIdentity {
        connection_id: "repo".to_owned(),
        intent_id: 1,
        generation: 1,
        role: "runner",
        expected_name: NAME,
        container_id: id,
        volume,
    }
}

fn labels(volume: &str, role: &str) -> serde_json::Value {
    serde_json::json!({
        "velnor.volume": volume,
        "velnor.worker": volume,
        "velnor.role": role
    })
}

fn record() -> DiagnosticRecord {
    DiagnosticRecord {
        version: 1,
        connection_id: "repo".to_owned(),
        intent_id: 1,
        claim_generation: 1,
        role: "runner".to_owned(),
        container_id: ID.to_owned(),
        name: NAME.to_owned(),
        image: Some("sha256:0123456789abcdef".to_owned()),
        restart_count: Some(3),
        state: None,
    }
}

fn private_dir(path: &PathBuf) -> Result<(), String> {
    fs::create_dir(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())
}

struct Scratch {
    path: PathBuf,
    directory: PathBuf,
}

impl Scratch {
    fn open(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "velnor-diagnostics-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path: fs::canonicalize(&directory).map_err(|error| error.to_string())?,
            directory,
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.directory)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            let _removed = fs::remove_file(&self.directory);
        } else {
            let _removed = fs::remove_dir_all(&self.directory);
        }
    }
}
