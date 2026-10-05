//! Exercises fail-closed behavior of the Scale Set bundle exporter.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use super::render_mbx;

static MBX_EXPORT_CASES: AtomicU64 = AtomicU64::new(0);

const DF_STUB: &str = "#!/bin/sh\nexit 0\n";

const MBX_STUB: &str = r#"#!/bin/sh
set -eu
if [ "${1:-}" = "gc" ]; then exit 0; fi
if [ "${1:-}" = "cache" ] && [ "${2:-}" = "dir" ]; then
  printf '%s\n' "$MBX_STUB_STORE"
  exit 0
fi
if [ "${1:-}" = "cache" ] && [ "${2:-}" = "export" ]; then
  bundle=
  for arg in "$@"; do bundle=$arg; done
  if [ "$MBX_STUB_MODE" = "nodir" ]; then
    exit 0
  fi
  mkdir -p "$bundle"
  if [ "$MBX_STUB_MODE" = "enospc" ]; then
    printf 'No space left on device\n'
    exit 1
  fi
  test -d "$MBX_STUB_STORE"
  test -f "$MBX_STUB_STORE/keep"
  printf 'ok\n' > "$bundle/payload"
  printf 'saw\n' > "$MBX_STUB_WITNESS"
  exit 0
fi
printf 'unexpected mbx\n' >&2
exit 2
"#;

struct CaseDir {
    root: PathBuf,
}

fn io<T>(result: std::io::Result<T>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

impl CaseDir {
    fn create(label: &str) -> Result<Self, String> {
        let n = MBX_EXPORT_CASES.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-mbx-export-{label}-{}-{n}",
            std::process::id()
        ));
        io(fs::create_dir_all(&root))?;
        Ok(Self { root })
    }
}

impl Drop for CaseDir {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.root) {
            eprintln!("case cleanup failed: {err}");
        }
    }
}

struct RunOut {
    output: Output,
    case: CaseDir,
}

fn write_exe(path: &Path, body: &str) -> Result<(), String> {
    io(fs::write(path, body))?;
    let mut perms = io(fs::metadata(path))?.permissions();
    perms.set_mode(0o755);
    io(fs::set_permissions(path, perms))
}

fn yaml_double_body(text: &str) -> Result<&str, String> {
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Ok(&text[..index]);
        }
    }
    Err("unclosed run scalar".to_owned())
}

fn unescape_yaml_double(scalar: &str) -> Result<String, String> {
    let mut out = String::with_capacity(scalar.len());
    let mut chars = scalar.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => return Err(format!("bad yaml escape {other}")),
            None => return Err("yaml trailing escape".to_owned()),
        }
    }
    Ok(out)
}

fn export_consumer_script(text: &str) -> Result<String, String> {
    let export = text
        .find("name: Export MBX single bundle")
        .ok_or_else(|| "export step".to_owned())?;
    let save_at = text[export..]
        .find("name: Save MBX single bundle")
        .ok_or_else(|| "save step".to_owned())?;
    let window = &text[export..export + save_at];
    let marker = "run: \"";
    let at = window.find(marker).ok_or_else(|| "run scalar".to_owned())?;
    let raw = yaml_double_body(&window[at + marker.len()..])?;
    let line = unescape_yaml_double(raw)?;
    let start = line.find("set -eu").ok_or_else(|| "set -eu".to_owned())?;
    let tail = &line[start..];
    let end = tail.find('\'').ok_or_else(|| "script close".to_owned())?;
    let script = &tail[..end];
    if script.starts_with("set -eu")
        && script.contains("mbx cache export")
        && script.contains(r#"rm -rf "$store""#)
        && script.contains(r#"echo "ready=true""#)
    {
        return Ok(script.to_owned());
    }
    Err(format!("not export script: {script}"))
}

fn run_rendered(script: &str, mode: &str) -> Result<RunOut, String> {
    let case = CaseDir::create(mode)?;
    let bin = case.root.join("bin");
    let store = case.root.join("mbx-store");
    io(fs::create_dir_all(&bin))?;
    io(fs::create_dir_all(&store))?;
    io(fs::write(store.join("keep"), "keep"))?;
    write_exe(&bin.join("df"), DF_STUB)?;
    write_exe(&bin.join("mbx"), MBX_STUB)?;
    io(fs::write(case.root.join("github-output"), ""))?;
    let script_path = case.root.join("export.sh");
    io(fs::write(&script_path, format!("{script}\n")))?;
    let bin_text = bin.to_str().ok_or_else(|| "utf-8 bin".to_owned())?;
    let path_env = std::env::var("PATH").map_err(|err| err.to_string())?;
    let output = io(Command::new("bash")
        .arg(&script_path)
        .current_dir(&case.root)
        .env("PATH", format!("{bin_text}:{path_env}"))
        .env("RUNNER_TEMP", &case.root)
        .env("MBX_CACHE_EXPORT_GROUP", "g")
        .env("GITHUB_OUTPUT", case.root.join("github-output"))
        .env("MBX_STUB_STORE", &store)
        .env("MBX_STUB_MODE", mode)
        .env("MBX_STUB_WITNESS", case.root.join("export-saw-store"))
        .output())?;
    Ok(RunOut { output, case })
}

fn assert_enospc(ran: &RunOut) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&ran.output.stdout);
    let stderr = String::from_utf8_lossy(&ran.output.stderr);
    let bundle = ran.case.root.join("mbx-single-bundle");
    let store = ran.case.root.join("mbx-store");
    assert!(!ran.output.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("No space left on device"),
        "{stdout}\n{stderr}"
    );
    assert!(!bundle.exists(), "bundle still present");
    assert!(store.is_dir(), "store missing");
    assert!(store.join("keep").is_file(), "store marker missing");
    assert!(!ran.case.root.join("export-saw-store").exists(), "{stdout}");
    let text = io(fs::read_to_string(ran.case.root.join("github-output")))?;
    assert!(!text.contains("ready=true"), "{text}");
    Ok(())
}

fn assert_export_without_directory(ran: &RunOut) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&ran.output.stdout);
    let stderr = String::from_utf8_lossy(&ran.output.stderr);
    let bundle = ran.case.root.join("mbx-single-bundle");
    let store = ran.case.root.join("mbx-store");
    assert!(!ran.output.status.success(), "{stdout}\n{stderr}");
    assert!(!bundle.exists(), "bundle still present");
    assert!(store.is_dir(), "store missing");
    assert!(store.join("keep").is_file(), "store marker missing");
    let text = io(fs::read_to_string(ran.case.root.join("github-output")))?;
    assert!(!text.contains("ready=true"), "ready output present");
    Ok(())
}

fn assert_saved(ran: &RunOut) -> Result<(), String> {
    let stdout = String::from_utf8_lossy(&ran.output.stdout);
    let stderr = String::from_utf8_lossy(&ran.output.stderr);
    let bundle = ran.case.root.join("mbx-single-bundle");
    let store = ran.case.root.join("mbx-store");
    assert!(ran.output.status.success(), "{stdout}\n{stderr}");
    assert!(ran.case.root.join("export-saw-store").is_file(), "{stderr}");
    assert!(!store.exists(), "store still present");
    assert!(!store.join("keep").exists(), "store marker still present");
    assert!(bundle.is_dir(), "bundle missing");
    assert!(bundle.join("payload").is_file(), "bundle payload missing");
    let text = io(fs::read_to_string(ran.case.root.join("github-output")))?;
    assert!(text.contains("ready=true"), "{text}");
    Ok(())
}

#[test]
fn scale_set_export_enospc_removes_bundle_and_success_deletes_store() -> Result<(), String> {
    let rendered = render_mbx("rust-demo__local", true).map_err(|err| err.to_string())?;
    let script = export_consumer_script(&rendered)?;
    assert_enospc(&run_rendered(&script, "enospc")?)?;
    assert_export_without_directory(&run_rendered(&script, "nodir")?)?;
    assert_saved(&run_rendered(&script, "ok")?)?;
    Ok(())
}
