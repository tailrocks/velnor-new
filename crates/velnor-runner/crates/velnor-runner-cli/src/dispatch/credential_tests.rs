use std::cell::{Cell, RefCell};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{CommandFactory, Parser};
use velnor_runner_host::HostError;

use super::connect::ConnectError;
use super::connection_state::persist_host;
use super::credential::import_release_read_with;
use crate::args::{Cli, Command, CredentialAction};

const CONNECTION_CREDENTIAL: &str = "keychain:com.tailrocks.velnor.host/example";
const DEFAULT_RELEASE_READ: &str = "keychain:com.tailrocks.velnor.host/release-read";
const CONFIG_WITHOUT_RELEASE_READ: &str = concat!(
    "schema = 2\n",
    "[[connections]]\n",
    "connection_id = \"example\"\n",
    "repository_id = 1\n",
    "repository = \"owner/repo\"\n",
    "credential_ref = \"keychain:com.tailrocks.velnor.host/example\"\n",
    "[connections.scale_set]\n",
    "name = \"example-set\"\n",
    "labels = [\"velnor\", \"example-set\"]\n",
    "[connections.source_policy]\n",
    "mode = \"branch_restricted\"\n",
    "branches = [\"main\"]\n",
    "tag_prefixes = []\n",
    "events = [\"push\"]\n",
    "pull_request_rules = []\n",
    "[host]\n",
    "max_jobs = 1\n",
    "[host.resources]\n",
    "runner_cpu_millicores = 1000\n",
    "runner_memory_bytes = 2147483648\n",
    "dind_cpu_millicores = 3000\n",
    "dind_memory_bytes = 6442450944\n",
    "operational_memory_reserve_bytes = 1073741824\n",
    "[docker]\n",
    "context = \"orbstack\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn command_has_no_secret_argument() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "credential",
        "import-release-read",
        "--state",
        "/tmp/velnor-state",
    ])
    .map_err(|error| error.to_string())?;
    if !matches!(
        cli.command,
        Command::Credential {
            action: CredentialAction::ImportReleaseRead
        }
    ) || cli.state.as_deref() != Some(Path::new("/tmp/velnor-state"))
    {
        return Err("credential command did not parse".to_owned());
    }
    if Cli::try_parse_from([
        "velnor-host",
        "credential",
        "import-release-read",
        "--token",
        "placeholder",
    ])
    .is_ok()
    {
        return Err("credential command accepted a secret argument".to_owned());
    }
    let mut command = Cli::command();
    let credential = command
        .find_subcommand_mut("credential")
        .ok_or("missing credential command")?;
    let import = credential
        .find_subcommand_mut("import-release-read")
        .ok_or("missing release-read action")?;
    let mut help = Vec::new();
    import
        .write_long_help(&mut help)
        .map_err(|error| error.to_string())?;
    let help = String::from_utf8(help).map_err(|error| error.to_string())?;
    if !help.contains("stdin") || help.contains("--token") {
        return Err("release-read help does not describe stdin".to_owned());
    }
    Ok(())
}

#[test]
fn import_uses_configured_release_reference_without_config_write() -> Result<(), String> {
    let dir = TempDir::new("configured")?;
    let initial = CONFIG_WITHOUT_RELEASE_READ.replace(
        "[host]\n",
        "[release_read]\ncredential_ref = \"keychain:com.tailrocks.velnor.host/release-audit\"\n[host]\n",
    );
    write_config(dir.path(), &initial)?;
    let stored = RefCell::new(None);
    let mut input = Cursor::new(b"release-canary\n");
    import_release_read_with(
        &mut input,
        dir.path(),
        |service, account, secret| {
            *stored.borrow_mut() = Some((service.to_owned(), account.to_owned(), secret.to_vec()));
            Ok(())
        },
        |_, _| Err(ConnectError::Write),
    )
    .map_err(|error| error.to_string())?;
    let stored = stored.into_inner().ok_or("store was not called")?;
    if stored.0 != "com.tailrocks.velnor.host"
        || stored.1 != "release-audit"
        || stored.2.as_slice() != b"release-canary\n"
        || read_config(dir.path())? != initial
    {
        return Err("configured release credential changed".to_owned());
    }
    Ok(())
}

#[test]
fn import_writes_default_reference_only_after_keychain_success() -> Result<(), String> {
    let dir = TempDir::new("default")?;
    write_config(dir.path(), CONFIG_WITHOUT_RELEASE_READ)?;
    let stored = RefCell::new(None);
    let mut input = Cursor::new(b"release-canary\n");
    import_release_read_with(
        &mut input,
        dir.path(),
        |service, account, secret| {
            *stored.borrow_mut() = Some((service.to_owned(), account.to_owned(), secret.to_vec()));
            Ok(())
        },
        persist_host,
    )
    .map_err(|error| error.to_string())?;
    let stored = stored.into_inner().ok_or("store was not called")?;
    let config = read_config(dir.path())?;
    if stored.0 != "com.tailrocks.velnor.host"
        || stored.1 != "release-read"
        || stored.2.as_slice() != b"release-canary\n"
        || !config.contains(&format!("credential_ref = \"{DEFAULT_RELEASE_READ}\""))
        || config.contains("release-canary")
    {
        return Err("default release credential was not stored".to_owned());
    }
    Ok(())
}

#[test]
fn import_collision_and_missing_config_do_not_store_or_write() -> Result<(), String> {
    let missing = TempDir::new("missing")?;
    let store_called = Cell::new(false);
    let persist_called = Cell::new(false);
    let mut input = Cursor::new(b"release-canary\n");
    if import_release_read_with(
        &mut input,
        missing.path(),
        |_, _, _| {
            store_called.set(true);
            Ok(())
        },
        |_, _| {
            persist_called.set(true);
            Ok(())
        },
    ) != Err(ConnectError::Config)
        || store_called.get()
        || persist_called.get()
    {
        return Err("missing config caused an effect".to_owned());
    }

    let collision = TempDir::new("collision")?;
    let changed = CONFIG_WITHOUT_RELEASE_READ.replace(CONNECTION_CREDENTIAL, DEFAULT_RELEASE_READ);
    write_config(collision.path(), &changed)?;
    let mut input = Cursor::new(b"release-canary\n");
    if import_release_read_with(
        &mut input,
        collision.path(),
        |_, _, _| {
            store_called.set(true);
            Ok(())
        },
        |_, _| {
            persist_called.set(true);
            Ok(())
        },
    ) != Err(ConnectError::Config)
        || store_called.get()
        || persist_called.get()
        || read_config(collision.path())? != changed
    {
        return Err("credential collision caused an effect".to_owned());
    }
    Ok(())
}

#[test]
fn bounded_stdin_and_keychain_failure_keep_config_unchanged() -> Result<(), String> {
    let dir = TempDir::new("failure")?;
    write_config(dir.path(), CONFIG_WITHOUT_RELEASE_READ)?;
    let initial = read_config(dir.path())?;
    let store_called = Cell::new(false);
    for bytes in [Vec::new(), vec![b'x'; 4097], vec![0xff]] {
        let mut input = Cursor::new(bytes);
        if import_release_read_with(
            &mut input,
            dir.path(),
            |_, _, _| {
                store_called.set(true);
                Ok(())
            },
            persist_host,
        )
        .is_ok()
            || store_called.get()
            || read_config(dir.path())? != initial
        {
            return Err("invalid stdin caused an effect".to_owned());
        }
    }

    let mut input = Cursor::new(b"release-canary\n");
    let persist_called = Cell::new(false);
    if import_release_read_with(
        &mut input,
        dir.path(),
        |_, _, _| Err(HostError::Keychain),
        |_, _| {
            persist_called.set(true);
            Ok(())
        },
    ) != Err(ConnectError::Secret(HostError::Keychain))
        || persist_called.get()
        || read_config(dir.path())? != initial
    {
        return Err("Keychain failure changed config".to_owned());
    }
    Ok(())
}

#[test]
fn config_write_failure_returns_error_after_keychain_store() -> Result<(), String> {
    let dir = TempDir::new("write-failure")?;
    write_config(dir.path(), CONFIG_WITHOUT_RELEASE_READ)?;
    let stored = Cell::new(false);
    let persisted = Cell::new(false);
    let mut input = Cursor::new(b"release-canary\n");
    let result = import_release_read_with(
        &mut input,
        dir.path(),
        |service, account, secret| {
            stored.set(
                service == "com.tailrocks.velnor.host"
                    && account == "release-read"
                    && secret == b"release-canary\n",
            );
            Ok(())
        },
        |_, text| {
            persisted.set(text.contains(&format!("credential_ref = \"{DEFAULT_RELEASE_READ}\"")));
            Err(ConnectError::Write)
        },
    );
    if result != Err(ConnectError::Write) || !stored.get() || !persisted.get() {
        return Err("config write failure reported success or skipped storage".to_owned());
    }
    Ok(())
}

fn write_config(state: &Path, text: &str) -> Result<(), String> {
    std::fs::write(state.join("host.toml"), text).map_err(|error| error.to_string())
}

fn read_config(state: &Path) -> Result<String, String> {
    std::fs::read_to_string(state.join("host.toml")).map_err(|error| error.to_string())
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut attempts = 0;
        loop {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "velnor-credential-{label}-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    attempts += 1;
                    if attempts == 100 {
                        return Err("could not create a temporary directory".to_owned());
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}
