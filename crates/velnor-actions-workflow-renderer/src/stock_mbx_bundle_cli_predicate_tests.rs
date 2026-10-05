use super::*;
use crate::stock_mbx_bundle::MBX_OBJECT_PREDICATE;
use std::process::Output;

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("remove predicate test root {}: {error}", self.0.display());
        }
    }
}

fn predicate_status(root: &Path) -> Result<i32, String> {
    let output = predicate_output(root, false)?;
    output
        .status
        .code()
        .ok_or_else(|| format!("predicate exited without a status: {}", text(&output)))
}

fn predicate_output(root: &Path, as_unprivileged: bool) -> Result<Output, String> {
    let script = format!("set -eu; {MBX_OBJECT_PREDICATE}; contains_cache_objects \"$1\"");
    let mut command = Command::new(if as_unprivileged {
        "/usr/bin/setpriv"
    } else {
        "/bin/sh"
    });
    if as_unprivileged {
        command.args([
            OsStr::new("--reuid=65534"),
            OsStr::new("--regid=65534"),
            OsStr::new("--clear-groups"),
            OsStr::new("/bin/sh"),
        ]);
    }
    command
        .args([
            OsStr::new("-c"),
            OsStr::new(&script),
            OsStr::new("predicate"),
        ])
        .arg(root)
        .env_clear()
        .env("RUNNER_OS", "Linux")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())
}

fn exercise_cross_device_descendant(root: &Path) -> Result<(), String> {
    let mountpoint = root.join("cas/v1/blake3/mounted-device");
    let mounted_file = root.join("cas/v1/blake3/mounted-file");
    fs::create_dir(&mountpoint).map_err(|err| err.to_string())?;
    fs::write(&mounted_file, b"mount target").map_err(|err| err.to_string())?;
    let source_root = root.with_file_name(format!(
        "{}-mount-source",
        root.file_name()
            .and_then(OsStr::to_str)
            .ok_or("predicate root name is not UTF-8")?
    ));
    fs::create_dir(&source_root).map_err(|err| err.to_string())?;
    let _source_root = TempRoot(source_root.clone());
    let source_mount = source_root.join("tmpfs");
    fs::create_dir(&source_mount).map_err(|err| err.to_string())?;
    let script = format!(
        "set -eu\ntrap 'umount \"$2/cas/v1/blake3/mounted-file\" 2>/dev/null || :; umount \"$3\" 2>/dev/null || :; umount \"$1\" 2>/dev/null || :' EXIT\nmount -t tmpfs -o size=4m,nr_inodes=32 tmpfs \"$1\"\nprintf foreign > \"$1/object\"\n{MBX_OBJECT_PREDICATE}\nstatus=0; contains_cache_objects \"$2\" || status=$?\nif [ \"$status\" -ne 2 ]; then printf 'mounted-directory predicate status=%s, expected unsafe status 2\\n' \"$status\" >&2; exit 42; fi\nprintf 'mounted-directory-rejected\\n'\numount \"$1\"\nif ! mount -t tmpfs -o size=4m,nr_inodes=32 tmpfs \"$3\"; then printf 'mounted-file-source-unavailable\\n'; exit 0; fi\nprintf foreign > \"$3/source\"\nif ! mount --bind \"$3/source\" \"$2/cas/v1/blake3/mounted-file\"; then printf 'mounted-file-bind-unavailable\\n'; exit 0; fi\nstatus=0; contains_cache_objects \"$2\" || status=$?\nif [ \"$status\" -ne 2 ]; then printf 'mounted-file predicate status=%s, expected unsafe status 2\\n' \"$status\" >&2; exit 43; fi\nprintf 'mounted-file-rejected\\n'"
    );
    let result = Command::new("/usr/bin/unshare")
        .args([
            OsStr::new("--user"),
            OsStr::new("--map-root-user"),
            OsStr::new("--mount"),
            OsStr::new("--propagation"),
            OsStr::new("private"),
            OsStr::new("--"),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new(&script),
            OsStr::new("predicate-mount"),
        ])
        .arg(&mountpoint)
        .arg(root)
        .arg(&source_mount)
        .env_clear()
        .env("RUNNER_OS", "Linux")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())?;
    let output = text(&result);
    if result.status.success()
        && output.contains("mounted-directory-rejected")
        && output.contains("mounted-file-rejected")
    {
        return Ok(());
    }
    if result.status.code() == Some(42) || result.status.code() == Some(43) {
        return Err(format!(
            "object predicate accepted a mounted descendant: {output}"
        ));
    }
    if result.status.success() && output.contains("mounted-directory-rejected") {
        return Err(format!(
            "required cross-device regular-file bind fixture unavailable; predicate result is incomplete: {output}"
        ));
    }
    let unavailable = output.to_ascii_lowercase();
    if unavailable.contains("operation not permitted")
        || unavailable.contains("unshare failed")
        || unavailable.contains("invalid argument")
        || unavailable.contains("permission denied")
    {
        return Err(format!(
            "required Linux user+mount namespace capability unavailable; predicate result is incomplete: {output}"
        ));
    }
    Err(format!("cross-device predicate fixture failed: {output}"))
}

fn effective_uid() -> Result<u32, String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|err| format!("cannot identify test uid: {err}"))
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_object_predicate_rejects_empty_symlink_and_hardlink_trees() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!(
        "velnor-mbx-object-predicate-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).map_err(|err| err.to_string())?;
    let _root = TempRoot(root.clone());
    let cas = root.join("cas/v1/blake3");
    let actions = root.join("action-results/v1/blake3");
    fs::create_dir_all(&cas).map_err(|err| err.to_string())?;
    fs::create_dir_all(&actions).map_err(|err| err.to_string())?;
    fs::write(root.join("manifest.json"), b"metadata-only bundle")
        .map_err(|err| err.to_string())?;
    if predicate_status(&root)? != 1 {
        return Err("metadata-only directories became save eligible".to_owned());
    }

    let cas_object = cas.join("object");
    let action_object = actions.join("object");
    fs::write(&cas_object, b"cas-object").map_err(|err| err.to_string())?;
    fs::write(&action_object, b"action-result").map_err(|err| err.to_string())?;
    if predicate_status(&root)? != 0 {
        return Err("regular cache objects were not recognized".to_owned());
    }
    exercise_cross_device_descendant(&root)?;

    let external = root.join("external-object");
    fs::write(&external, b"outside-object").map_err(|err| err.to_string())?;
    symlink(&external, cas.join("linked-object")).map_err(|err| err.to_string())?;
    if predicate_status(&root)? != 2 {
        return Err("a symlink descendant was accepted beside a real object".to_owned());
    }
    fs::remove_file(cas.join("linked-object")).map_err(|err| err.to_string())?;

    fs::remove_file(&cas_object).map_err(|err| err.to_string())?;
    fs::hard_link(&external, &cas_object).map_err(|err| err.to_string())?;
    if predicate_status(&root)? != 2 {
        return Err("a hard-linked object was accepted".to_owned());
    }

    fs::remove_file(&cas_object).map_err(|err| err.to_string())?;
    fs::write(&cas_object, b"cas-object").map_err(|err| err.to_string())?;
    let unreadable = root.join("unreadable-descendant");
    fs::create_dir(&unreadable).map_err(|err| err.to_string())?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).map_err(|err| err.to_string())?;
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000))
        .map_err(|err| err.to_string())?;
    let dropped = effective_uid()? == 0;
    let traversal = predicate_output(&root, dropped);
    let child_permissions = fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o700));
    let root_permissions = fs::set_permissions(&root, fs::Permissions::from_mode(0o700));
    child_permissions.map_err(|err| err.to_string())?;
    root_permissions.map_err(|err| err.to_string())?;
    let traversal = traversal?;
    let diagnostic = String::from_utf8_lossy(&traversal.stderr).to_ascii_lowercase();
    if traversal.status.code() != Some(2) || !diagnostic.contains("permission denied") {
        return Err(format!(
            "object predicate did not fail closed on traversal error: {diagnostic}"
        ));
    }
    Ok(())
}
