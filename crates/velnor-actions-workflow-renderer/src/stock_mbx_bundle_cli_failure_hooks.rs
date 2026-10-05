use super::*;
use std::os::unix::fs::PermissionsExt;

pub(super) fn inject_enospc(stage: &Stage) -> Result<(), String> {
    let executable = stage.root.join("bin/mbx");
    fs::remove_file(&executable).map_err(|err| err.to_string())?;
    fs::write(&executable, "#!/bin/sh\nif [ \"${1:-}\" = cache ] && [ \"${2:-}\" = export ]; then printf '%s\\n' 'injected: No space left on device' >&2; exit 28; fi\nexec \"$VELNOR_MBX_REAL\" \"$@\"\n").map_err(|err| err.to_string())?;
    fs::set_permissions(executable, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())
}

struct BarrierFiles {
    marker: PathBuf,
    gate: PathBuf,
    hook: PathBuf,
    target: PathBuf,
}

fn barrier_files(stage: &Stage, label: &str) -> Result<BarrierFiles, String> {
    let marker = stage.root.join(label);
    let gate = stage.root.join(format!("{label}.release"));
    let hook = stage.root.join(format!("bin/{label}-cargo-barrier"));
    let target = stage.root.join(format!("{label}-producer-target"));
    fs::create_dir(&target).map_err(|err| err.to_string())?;
    fs::write(target.join("sentinel"), b"producer target stays untouched")
        .map_err(|err| err.to_string())?;
    let status = Command::new("/usr/bin/mkfifo")
        .arg(&gate)
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        return Err(format!("cannot create Cargo release FIFO: {status}"));
    }
    fs::set_permissions(&gate, fs::Permissions::from_mode(0o600)).map_err(|err| err.to_string())?;
    Ok(BarrierFiles {
        marker,
        gate,
        hook,
        target,
    })
}

fn barrier_environment<'a>(files: &'a BarrierFiles, cargo: &'a Path) -> [(&'a str, &'a OsStr); 5] {
    [
        ("CARGO", files.hook.as_os_str()),
        ("VELNOR_OFFICIAL_CARGO", cargo.as_os_str()),
        ("VELNOR_METADATA_MARKER", files.marker.as_os_str()),
        ("VELNOR_METADATA_GATE", files.gate.as_os_str()),
        ("CARGO_TARGET_DIR", files.target.as_os_str()),
    ]
}

fn validate_barrier_observation(
    fields: &BTreeMap<String, String>,
    cargo_argv: &[String],
    mbx_argv: &[String],
    cargo_env: &BTreeMap<String, String>,
    stage: &Stage,
    hook: &Path,
) -> Result<(), String> {
    let expected_cargo = ["metadata", "--no-deps", "--format-version", "1"];
    let expected_mbx = [
        "cache",
        "export",
        "--group",
        stage.group.as_str(),
        "--format",
        "directory",
        stage.bundle.to_str().ok_or("bundle path is not UTF-8")?,
    ];
    let mbx_path = stage.root.join("bin/mbx");
    let official_mbx = stage.binary.to_string_lossy();
    let mbx_program_matches = mbx_argv.first().is_some_and(|program| {
        program == "mbx"
            || program == official_mbx.as_ref()
            || Path::new(program) == mbx_path.as_path()
    });
    let shim_pid = fields.get("shim_pid").ok_or("barrier omitted shim PID")?;
    let mbx_pid = fields.get("mbx_pid").ok_or("barrier omitted MBX PID")?;
    let pgid = fields.get("pgid").ok_or("barrier omitted process group")?;
    if !arguments_match(cargo_argv, &expected_cargo)
        || !mbx_program_matches
        || !mbx_argv
            .get(1..)
            .is_some_and(|arguments| arguments_match(arguments, &expected_mbx))
        || fields.get("pgid") != Some(fields.get("mbx_pgid").ok_or("barrier omitted MBX PGID")?)
        || pgid != mbx_pid
        || shim_pid == mbx_pid
        || fields.get("cwd").map(String::as_str)
            != Some(stage.import_cwd.to_str().ok_or("cwd is not UTF-8")?)
        || cargo_env.get("CARGO").map(String::as_str) != hook.to_str()
        || cargo_env.get("CARGO_HOME").map(String::as_str)
            != Some(
                stage
                    .root
                    .join("cargo")
                    .to_str()
                    .ok_or("Cargo home path is not UTF-8")?,
            )
        || cargo_env.contains_key("CARGO_TARGET_DIR")
        || cargo_env.get("MBX_CACHE_DIR").map(String::as_str)
            != Some(stage.primary.to_str().ok_or("store path is not UTF-8")?)
        || cargo_env.get("MBX_CACHE_EXPORT_GROUP").map(String::as_str) != Some(stage.group.as_str())
        || cargo_env.get("RUSTUP_TOOLCHAIN").map(String::as_str) != Some("1.98.1")
    {
        return Err(format!(
            "CARGO barrier did not observe MBX's exact metadata call: fields={fields:?} cargo_argv={cargo_argv:?} mbx_argv={mbx_argv:?} env={cargo_env:?}"
        ));
    }
    Ok(())
}

fn write_metadata_workspace(root: &Path) -> Result<(), String> {
    fs::create_dir(root.join("src")).map_err(|err| err.to_string())?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"mbx-metadata-barrier-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .map_err(|err| err.to_string())?;
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").map_err(|err| err.to_string())
}

fn write_cargo_barrier(path: &Path) -> Result<(), String> {
    let script = r#"#!/bin/sh
set -eu
marker=${VELNOR_METADATA_MARKER:?}
mbx_pid=$PPID
pgid=$(ps -o pgid= -p "$$" | tr -d '[:space:]')
mbx_pgid=$(ps -o pgid= -p "$mbx_pid" | tr -d '[:space:]')
printf '%s\0' "$@" > "$marker.argv"
cat "/proc/$mbx_pid/cmdline" > "$marker.mbx-argv"
readlink "/proc/$mbx_pid/exe" > "$marker.mbx-exe"
/usr/bin/env -0 > "$marker.env"
tmp="$marker.tmp"
printf 'shim_pid=%s\nmbx_pid=%s\npgid=%s\nmbx_pgid=%s\ncwd=%s\n' "$$" "$mbx_pid" "$pgid" "$mbx_pgid" "$(pwd -P)" > "$tmp"
mv "$tmp" "$marker"
IFS= read -r release < "$VELNOR_METADATA_GATE"
[ "$release" = release ]
set +e
"$VELNOR_OFFICIAL_CARGO" "$@" > "$marker.metadata.json" 2> "$marker.metadata.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$marker.status"
cat "$marker.metadata.json"
cat "$marker.metadata.stderr" >&2
exit "$status"
"#;
    fs::write(path, script).map_err(|err| err.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|err| err.to_string())
}

pub(super) fn exercise_metadata_cancellation(
    fixture: &Fixture,
    lifecycle: &MbxBundleCacheSteps,
) -> Result<(), String> {
    exercise_released_metadata_control(fixture, lifecycle)?;
    exercise_cancelled_metadata_barrier(fixture, lifecycle)
}

fn exercise_released_metadata_control(
    fixture: &Fixture,
    lifecycle: &MbxBundleCacheSteps,
) -> Result<(), String> {
    let stage = fixture.stage("metadata-release", "810014", false)?;
    let (miss, selected) = import_miss(lifecycle, &stage)?;
    check_empty_store(&stage, &selected)?;
    write_metadata_workspace(&stage.import_cwd)?;
    let files = barrier_files(&stage, "released-cargo")?;
    write_cargo_barrier(&files.hook)?;
    let before = tree_hash(&files.target)?;
    let cargo = tool_identity::official_cargo_binary(&stage)?;
    let extra = barrier_environment(&files, &cargo);
    let observed = run_shell_gated(&GatedShell {
        step: &lifecycle.save[0],
        stage: &stage,
        outputs: &miss.outputs,
        cwd: &stage.project,
        temp: &stage.temp,
        extra: &extra,
        marker: &files.marker,
        label: "released-cargo-metadata",
        action: GateAction::Release(&files.gate),
    })?;
    let metadata_path = files.marker.with_extension("metadata.json");
    let metadata = fs::read_to_string(&metadata_path).map_err(|error| {
        format!(
            "released Cargo metadata file is missing: {error}; marker={}; argv={}; env={}; cargo_stderr={}; shell={}",
            diagnostic_file(&files.marker),
            diagnostic_file(&files.marker.with_extension("argv")),
            diagnostic_file(&files.marker.with_extension("env")),
            diagnostic_file(&files.marker.with_extension("metadata.stderr")),
            observed.step.text
        )
    })?;
    let status = read_status(&files.marker.with_extension("status"))?;
    let after = tree_hash(&files.target)?;
    if status != 0
        || !metadata.starts_with('{')
        || !metadata.contains("\"packages\"")
        || !metadata.contains("mbx-metadata-barrier-fixture")
        || observed.timed_out
        || !observed.step.success
        || observed.step.outputs.get("ready").map(String::as_str) != Some("false")
        || before != after
    {
        return Err(format!(
            "released official Cargo metadata control failed: status={status}, output={metadata}, step={:?}",
            observed.step
        ));
    }
    check_barrier_outputs(&files.marker, &stage, &cargo, &files.hook)?;
    cleanup_after_export(lifecycle, &stage, &observed.step.outputs)?;
    eprintln!(
        "CARGO barrier release control: official_cargo={} status={status} metadata_bytes={} cwd={} target_unchanged={}",
        cargo.display(),
        metadata.len(),
        stage.import_cwd.display(),
        before == after
    );
    Ok(())
}

fn exercise_cancelled_metadata_barrier(
    fixture: &Fixture,
    lifecycle: &MbxBundleCacheSteps,
) -> Result<(), String> {
    let stage = fixture.stage("metadata-cancel", "810015", false)?;
    let (miss, selected) = import_miss(lifecycle, &stage)?;
    check_empty_store(&stage, &selected)?;
    write_metadata_workspace(&stage.import_cwd)?;
    let files = barrier_files(&stage, "cancelled-cargo")?;
    write_cargo_barrier(&files.hook)?;
    let before = tree_hash(&files.target)?;
    let cargo = tool_identity::official_cargo_binary(&stage)?;
    let extra = barrier_environment(&files, &cargo);
    let cancelled = run_shell_gated(&GatedShell {
        step: &lifecycle.save[0],
        stage: &stage,
        outputs: &miss.outputs,
        cwd: &stage.project,
        temp: &stage.temp,
        extra: &extra,
        marker: &files.marker,
        label: "cancelled-cargo-metadata",
        action: GateAction::Cancel,
    })?;
    let fields = key_values(&cancelled.hook)?;
    let group = fields.get("pgid").ok_or("CARGO barrier omitted PGID")?;
    let after = tree_hash(&files.target)?;
    let live_group = group_has_live_processes(group)?;
    let target_changed = after != before;
    let status_written = files.marker.with_extension("status").exists();
    let metadata_written = files.marker.with_extension("metadata.json").exists();
    let cleanup_safe = cancelled
        .step
        .outputs
        .get("cleanup_safe")
        .map(String::as_str);
    let ready = cancelled.step.outputs.get("ready").map(String::as_str);
    if live_group
        || target_changed
        || cancelled.timed_out
        || cancelled.exit_code != Some(143)
        || cancelled.step.success
        || cleanup_safe != Some("true")
        || ready == Some("true")
        || status_written
        || metadata_written
    {
        return Err(format!(
            "instrumented CARGO cancellation failed: group={group}, live_group={live_group}, target_changed={target_changed}, timed_out={}, exit_code={:?}, step_success={}, cleanup_safe={cleanup_safe:?}, ready={ready:?}, status_written={status_written}, metadata_written={metadata_written}, step={:?}",
            cancelled.timed_out, cancelled.exit_code, cancelled.step.success, cancelled.step
        ));
    }
    check_barrier_outputs(&files.marker, &stage, &cargo, &files.hook)?;
    let mut cleanup_inputs = cancelled.step.outputs.clone();
    cleanup_inputs.insert("export_outcome".to_owned(), "cancelled".to_owned());
    cleanup_inputs.insert("import_outcome".to_owned(), "success".to_owned());
    cleanup_inputs.insert("restore_outcome".to_owned(), "success".to_owned());
    cleanup_after_export(lifecycle, &stage, &cleanup_inputs)?;
    eprintln!(
        "instrumented cancellation: CARGO shim captured and blocked before official Cargo exec; group={group} quiescent=true target_unchanged=true official_cargo={}",
        cargo.display()
    );
    Ok(())
}

fn check_barrier_outputs(
    marker: &Path,
    stage: &Stage,
    cargo: &Path,
    hook: &Path,
) -> Result<(), String> {
    let fields = key_values(&fs::read_to_string(marker).map_err(|err| err.to_string())?)?;
    let argv = nul_argv(&marker.with_extension("argv"))?;
    let mbx_argv = nul_argv(&marker.with_extension("mbx-argv"))?;
    let env = nul_environment(&marker.with_extension("env"))?;
    validate_barrier_observation(&fields, &argv, &mbx_argv, &env, stage, hook)?;
    let mbx_executable =
        fs::read_to_string(marker.with_extension("mbx-exe")).map_err(|err| err.to_string())?;
    if Path::new(mbx_executable.trim())
        .canonicalize()
        .map_err(|err| err.to_string())?
        != stage.binary
    {
        return Err(format!(
            "metadata probe used unexpected MBX executable: {mbx_executable}"
        ));
    }
    let real_cargo = env.get("VELNOR_OFFICIAL_CARGO").map(String::as_str);
    if real_cargo != cargo.to_str() {
        return Err(format!(
            "barrier did not bind official Cargo: {real_cargo:?}"
        ));
    }
    Ok(())
}

fn nul_argv(path: &Path) -> Result<Vec<String>, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8(argument.to_vec()).map_err(|err| err.to_string()))
        .collect()
}

fn nul_environment(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            let text = String::from_utf8(field.to_vec()).map_err(|err| err.to_string())?;
            let (key, value) = text
                .split_once('=')
                .ok_or_else(|| format!("malformed CARGO environment value: {text}"))?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn arguments_match(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

fn key_values(text: &str) -> Result<BTreeMap<String, String>, String> {
    text.lines()
        .map(|line| {
            line.split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .ok_or_else(|| format!("malformed metadata barrier field: {line}"))
        })
        .collect()
}

fn optional_status(path: &Path) -> Result<Option<i32>, String> {
    match fs::read_to_string(path) {
        Ok(value) => value
            .trim()
            .parse()
            .map(Some)
            .map_err(|err| format!("invalid Cargo status: {err}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn diagnostic_file(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) => format!("<unavailable: {error}>"),
    }
}

fn read_status(path: &Path) -> Result<i32, String> {
    optional_status(path)?.ok_or_else(|| "released Cargo omitted its status".to_owned())
}

fn group_has_live_processes(group: &str) -> Result<bool, String> {
    let listing = Command::new("/usr/bin/ps")
        .args(["-A", "-o", "pgid=,stat="])
        .output()
        .map_err(|err| err.to_string())?;
    if !listing.status.success() {
        return Err("cannot inspect cancellation process group".to_owned());
    }
    Ok(String::from_utf8_lossy(&listing.stdout)
        .lines()
        .any(|line| {
            let mut fields = line.split_whitespace();
            fields.next() == Some(group)
                && fields.next().is_some_and(|state| !state.starts_with('Z'))
        }))
}
