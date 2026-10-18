//! Per-user `LaunchAgent` install. The daemon stays in the foreground.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_host::launch_agent_plist;

use crate::args::ServiceAction;

const LABEL: &str = "com.tailrocks.velnor.host";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(crate) fn service(action: ServiceAction, state: &Path) -> ExitCode {
    match action {
        ServiceAction::Install => install(state),
        ServiceAction::Start => start(state),
        ServiceAction::Stop => stop(),
        ServiceAction::Uninstall => uninstall(),
    }
}

fn start(state: &Path) -> ExitCode {
    let Some(uid) = read_uid() else {
        return ExitCode::from(1);
    };
    let Ok(bin) = std::env::current_exe() else {
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(log_dir()).is_err()
        || !plist_matches(&plist_path(), &bin, state, &log_dir())
    {
        eprintln!("Service configuration is different.");
        return ExitCode::from(1);
    }
    spawn(&bootstrap_argv(uid, &plist_path()))
}

fn stop() -> ExitCode {
    let Some(uid) = read_uid() else {
        return ExitCode::from(1);
    };
    spawn(&bootout_argv(uid))
}

fn uninstall() -> ExitCode {
    uninstall_after_stop(stop(), &plist_path())
}

fn uninstall_after_stop(stopped: ExitCode, path: &Path) -> ExitCode {
    if stopped != ExitCode::SUCCESS {
        return ExitCode::from(1);
    }
    match std::fs::remove_file(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

fn install(state: &Path) -> ExitCode {
    let Ok(bin) = std::env::current_exe() else {
        return ExitCode::from(1);
    };
    let path = plist_path();
    if !write_plist(&bin, state, &path, &log_dir()) {
        return ExitCode::from(1);
    }
    println!("{}", path.display());
    ExitCode::SUCCESS
}

fn spawn(argv: &[String]) -> ExitCode {
    let Some((bin, rest)) = argv.split_first() else {
        return ExitCode::from(1);
    };
    if bin.is_empty() {
        return ExitCode::from(1);
    }
    match Command::new(bin).args(rest).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    }
}

fn read_uid() -> Option<u32> {
    let output = Command::new("id").arg("-u").output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let uid = text.trim().parse().ok()?;
    if uid == 0 { None } else { Some(uid) }
}

fn write_plist(binary: &Path, state: &Path, path: &Path, logs: &Path) -> bool {
    if std::fs::create_dir_all(state).is_err() {
        return false;
    }
    let Some(body) = render_plist(binary, state, logs) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err()
        || std::fs::create_dir_all(logs).is_err()
        || !regular_file_or_absent(path)
    {
        return false;
    }
    atomic_write(path, body.as_bytes()).is_ok()
}

fn regular_file_or_absent(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if existing_matches(path, contents)? {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let staging = PrivateDirectory::create(parent, "velnor-plist-stage")?;
    let temporary = staging.path().join("payload");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        drop(file);
        return Err(error);
    }
    drop(file);
    publish_staged(&temporary, path, contents)?;
    staging.cleanup()?;
    std::fs::File::open(parent)?.sync_all()
}

fn publish_staged(temporary: &Path, path: &Path, contents: &[u8]) -> std::io::Result<()> {
    match std::fs::hard_link(temporary, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if existing_matches(path, contents)? {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

struct PrivateDirectory {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl PrivateDirectory {
    fn create(parent: &Path, label: &str) -> std::io::Result<Self> {
        for _ in 0..16 {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".{label}-{}-{sequence}", std::process::id()));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    let metadata = std::fs::symlink_metadata(&path)?;
                    if !metadata.is_dir()
                        || metadata.file_type().is_symlink()
                        || metadata.uid() != rustix::process::geteuid().as_raw()
                        || metadata.mode() & 0o7777 != 0o700
                    {
                        drop(std::fs::remove_dir(&path));
                        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                    }
                    return Ok(Self {
                        path,
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(&self) -> std::io::Result<()> {
        let metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o7777 != 0o700
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
        {
            return Ok(());
        }
        std::fs::remove_dir_all(&self.path)
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        drop(self.cleanup());
    }
}

fn existing_matches(path: &Path, contents: &[u8]) -> std::io::Result<bool> {
    let path_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::NONBLOCK
        | rustix::fs::OFlags::CLOEXEC;
    let descriptor = rustix::fs::open(path, flags, rustix::fs::Mode::empty())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
    let mut file = std::fs::File::from(descriptor);
    let metadata = file.metadata()?;
    if !safe_plist_metadata(&metadata) || !same_file(&path_metadata, &metadata) {
        return Ok(false);
    }
    let expected_size = u64::try_from(contents.len())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    if metadata.len() != expected_size {
        return Ok(false);
    }
    let mut existing = Vec::with_capacity(contents.len());
    Read::by_ref(&mut file)
        .take(expected_size.saturating_add(1))
        .read_to_end(&mut existing)?;
    let current_path = std::fs::symlink_metadata(path)?;
    let current_file = file.metadata()?;
    Ok(existing == contents
        && current_path.is_file()
        && !current_path.file_type().is_symlink()
        && safe_plist_metadata(&current_file)
        && same_file(&current_path, &current_file))
}

fn safe_plist_metadata(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.mode() & 0o7777 == 0o600
}

fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn render_plist(binary: &Path, state: &Path, logs: &Path) -> Option<String> {
    let binary = binary
        .to_str()
        .filter(|text| !text.chars().any(xml_forbidden))?;
    let binary = Path::new(binary);
    let state = std::fs::canonicalize(state).ok()?;
    let plist = launch_agent_plist(binary).ok()?;
    let plist = with_state(&plist, &state)?;
    with_logs(&plist, logs)
}

fn plist_matches(path: &Path, binary: &Path, state: &Path, logs: &Path) -> bool {
    if !regular_file_or_absent(path) {
        return false;
    }
    let Some(expected) = render_plist(binary, state, logs) else {
        return false;
    };
    existing_matches(path, expected.as_bytes()).unwrap_or(false)
}

fn with_state(plist: &str, state: &Path) -> Option<String> {
    let state = state
        .to_str()
        .filter(|text| !text.chars().any(xml_forbidden))?;
    let marker = "<string>daemon</string>";
    let (before, after) = plist.split_once(marker)?;
    Some(format!(
        "{before}<string>--state</string><string>{}</string>{marker}{after}",
        xml_text(state)
    ))
}

pub(crate) fn log_dir() -> PathBuf {
    home().join("Library/Logs/Velnor")
}

fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn bootstrap_argv(uid: u32, plist: &Path) -> Vec<String> {
    vec![
        "launchctl".to_owned(),
        "bootstrap".to_owned(),
        format!("gui/{uid}"),
        plist.display().to_string(),
    ]
}

fn bootout_argv(uid: u32) -> Vec<String> {
    vec![
        "launchctl".to_owned(),
        "bootout".to_owned(),
        format!("gui/{uid}/{LABEL}"),
    ]
}

fn with_logs(plist: &str, log_dir: &Path) -> Option<String> {
    let out = log_dir.join("host.log");
    let err = log_dir.join("host.err.log");
    let out = out
        .to_str()
        .filter(|text| !text.chars().any(xml_forbidden))?;
    let err = err
        .to_str()
        .filter(|text| !text.chars().any(xml_forbidden))?;
    let keys = format!(
        "<key>StandardOutPath</key><string>{}</string>\n<key>StandardErrorPath</key><string>{}</string>\n</dict>",
        xml_text(out),
        xml_text(err)
    );
    Some(plist.replacen("</dict>", &keys, 1))
}

fn xml_forbidden(ch: char) -> bool {
    !matches!(
        u32::from(ch),
        0x20..=0xD7FF | 0xE000..=0xFFFD | 0x0001_0000..=0x0010_FFFF
    )
}

fn xml_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use super::{
        PrivateDirectory, bootout_argv, bootstrap_argv, plist_matches, publish_staged, same_file,
        uninstall_after_stop, with_logs, with_state, write_plist,
    };
    use std::process::ExitCode;

    #[test]
    fn launchctl_uses_the_gui_domain_and_does_not_fork() {
        let plist =
            Path::new("/Users/example/Library/LaunchAgents/com.tailrocks.velnor.host.plist");
        assert_eq!(
            bootstrap_argv(501, plist),
            vec![
                "launchctl".to_owned(),
                "bootstrap".to_owned(),
                "gui/501".to_owned(),
                plist.display().to_string(),
            ]
        );
        assert_eq!(
            bootout_argv(501),
            vec![
                "launchctl".to_owned(),
                "bootout".to_owned(),
                "gui/501/com.tailrocks.velnor.host".to_owned(),
            ]
        );
    }

    #[test]
    fn plist_logs_stay_out_of_the_secret_channel() {
        let body = with_logs(
            "<dict>\n</dict>\n",
            Path::new("/Users/example/Library/Logs/Velnor"),
        )
        .unwrap_or_default();
        assert!(body.contains("StandardOutPath"));
        assert!(body.contains("/Users/example/Library/Logs/Velnor/host.log"));
        assert!(!body.contains("ACTIONS_RUNNER_INPUT_JITCONFIG"));
        assert!(!body.contains("daemon fork"));
    }

    #[test]
    fn plist_runs_the_daemon_with_the_global_state_path() -> Result<(), String> {
        let plist = velnor_runner_host::launch_agent_plist(Path::new("/opt/velnor-host"))
            .map_err(|error| error.to_string())?;
        let body = with_state(&plist, Path::new("/tmp/velnor&<state>"))
            .ok_or_else(|| "state argument missing".to_owned())?;
        assert!(body.contains(
            "<string>/opt/velnor-host</string>\n<string>--state</string><string>/tmp/velnor&amp;&lt;state&gt;</string><string>daemon</string>"
        ));
        Ok(())
    }

    #[test]
    fn plist_rejects_a_state_path_that_xml_cannot_preserve() -> Result<(), String> {
        let plist = velnor_runner_host::launch_agent_plist(Path::new("/opt/velnor-host"))
            .map_err(|error| error.to_string())?;
        assert!(with_state(&plist, Path::new("/tmp/velnor\nstate")).is_none());
        assert!(with_state(&plist, Path::new("/tmp/velnor\u{0001}state")).is_none());
        assert!(with_logs("<dict></dict>", Path::new("/tmp/velnor\u{0001}logs")).is_none());
        Ok(())
    }

    fn test_scratch(label: &str) -> Result<super::PrivateDirectory, String> {
        super::PrivateDirectory::create(&std::env::temp_dir(), label)
            .map_err(|error| error.to_string())
    }

    #[test]
    fn service_start_requires_the_installed_state_argument() -> Result<(), String> {
        let scratch = test_scratch("velnor-service-start-test")?;
        let root = scratch.path();
        let state = root.join("state");
        let logs = root.join("logs");
        let other_state = root.join("other-state");
        let plist_path = root.join("agent.plist");
        let binary = Path::new("/opt/velnor-host");
        let result = write_plist(binary, &state, &plist_path, &logs);
        std::fs::create_dir_all(&other_state).map_err(|error| error.to_string())?;
        if !result
            || !plist_matches(&plist_path, binary, &state, &logs)
            || plist_matches(&plist_path, binary, &other_state, &logs)
        {
            return Err("service did not bind start to the installed state".to_owned());
        }
        let text = std::fs::read_to_string(&plist_path).map_err(|error| error.to_string())?;
        if !text.contains("<string>/opt/velnor-host</string>\n<string>--state</string><string>")
            || !text.contains("<string>daemon</string>\n<string>run</string>")
        {
            return Err("LaunchAgent arguments are not exact".to_owned());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn plist_install_does_not_follow_a_symlink() -> Result<(), String> {
        let scratch = test_scratch("velnor-service-link-test")?;
        let root = scratch.path();
        let state = root.join("state");
        let logs = root.join("logs");
        let target = root.join("target.plist");
        let path = root.join("agent.plist");
        std::fs::write(&target, b"keep this target").map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(&target, &path).map_err(|error| error.to_string())?;
        let written = write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs);
        let target_text = std::fs::read(&target).map_err(|error| error.to_string())?;
        let path_is_link = std::fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink();
        if written || target_text != b"keep this target" || !path_is_link {
            return Err("plist install followed a symlink".to_owned());
        }
        Ok(())
    }

    #[test]
    fn plist_install_preserves_a_different_existing_file() -> Result<(), String> {
        let scratch = test_scratch("velnor-service-conflict-test")?;
        let root = scratch.path();
        let state = root.join("state");
        let logs = root.join("logs");
        let path = root.join("agent.plist");
        let original = b"another LaunchAgent owns this path";
        std::fs::write(&path, original).map_err(|error| error.to_string())?;

        let written = write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs);
        let after = std::fs::read(&path).map_err(|error| error.to_string())?;
        if written || after != original {
            return Err("install replaced a different LaunchAgent file".to_owned());
        }
        Ok(())
    }

    #[test]
    fn plist_install_is_idempotent_for_its_exact_private_file() -> Result<(), String> {
        let scratch = test_scratch("velnor-service-idempotent-test")?;
        let root = scratch.path();
        let state = root.join("state");
        let logs = root.join("logs");
        let path = root.join("agent.plist");
        let binary = Path::new("/opt/velnor-host");
        if !write_plist(binary, &state, &path, &logs) {
            return Err("first install failed".to_owned());
        }
        let before = std::fs::metadata(&path).map_err(|error| error.to_string())?;
        let installed = write_plist(binary, &state, &path, &logs);
        let after = std::fs::metadata(&path).map_err(|error| error.to_string())?;
        if !installed || !same_file(&before, &after) || after.mode() & 0o777 != 0o600 {
            return Err("exact owned plist was not preserved idempotently".to_owned());
        }
        Ok(())
    }

    #[test]
    fn plist_install_rejects_matching_content_with_unsafe_permissions() -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;

        let scratch = test_scratch("velnor-service-mode-test")?;
        let root = scratch.path();
        let state = root.join("state");
        let logs = root.join("logs");
        let path = root.join("agent.plist");
        let binary = Path::new("/opt/velnor-host");
        if !write_plist(binary, &state, &path, &logs) {
            return Err("first install failed".to_owned());
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .map_err(|error| error.to_string())?;
        let unchanged = std::fs::read(&path).map_err(|error| error.to_string())?;
        let installed = write_plist(binary, &state, &path, &logs);
        let after = std::fs::read(&path).map_err(|error| error.to_string())?;
        if installed || after != unchanged {
            return Err("install accepted or changed a plist with unsafe permissions".to_owned());
        }
        Ok(())
    }

    #[test]
    fn racing_foreign_file_wins_without_being_unlinked() -> Result<(), String> {
        let scratch = test_scratch("velnor-service-race-test")?;
        let stage =
            PrivateDirectory::create(scratch.path(), "stage").map_err(|error| error.to_string())?;
        let staged = stage.path().join("payload");
        let destination = scratch.path().join("agent.plist");
        let expected = b"Velnor's plist";
        let foreign = b"foreign process won the publication race";
        std::fs::write(&staged, expected).map_err(|error| error.to_string())?;
        std::fs::write(&destination, foreign).map_err(|error| error.to_string())?;

        let result = publish_staged(&staged, &destination, expected);
        let after = std::fs::read(&destination).map_err(|error| error.to_string())?;
        if result.is_ok() || after != foreign {
            return Err("a racing file was replaced or removed".to_owned());
        }
        Ok(())
    }

    #[test]
    fn uninstall_keeps_the_plist_when_launchctl_stop_fails() -> Result<(), String> {
        let scratch = test_scratch("velnor-uninstall-test")?;
        let path = scratch.path().join("agent.plist");
        std::fs::write(&path, b"owned plist").map_err(|error| error.to_string())?;
        let result = uninstall_after_stop(ExitCode::from(1), &path);
        let exists = path.is_file();
        if result != ExitCode::from(1) || !exists {
            return Err("uninstall hid a failed stop".to_owned());
        }
        Ok(())
    }
}
