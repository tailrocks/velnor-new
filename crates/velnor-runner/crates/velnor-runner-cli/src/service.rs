//! Per-user `LaunchAgent` install. The daemon stays in the foreground.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_host::launch_agent_plist;

use crate::args::ServiceAction;

const LABEL: &str = "com.tailrocks.velnor.host";

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
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?
        .to_string_lossy();
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
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
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::File::open(parent)?.sync_all()
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
    if !regular_file_or_absent(path) || !path.is_file() {
        return false;
    }
    let Some(expected) = render_plist(binary, state, logs) else {
        return false;
    };
    std::fs::read_to_string(path).is_ok_and(|actual| actual == expected)
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
    !matches!(u32::from(ch), 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
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
    use std::path::Path;

    use super::{
        bootout_argv, bootstrap_argv, plist_matches, uninstall_after_stop, with_logs, with_state,
        write_plist,
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

    #[test]
    fn service_start_requires_the_installed_state_argument() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("velnor-service-{}", std::process::id()));
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
            let _ = std::fs::remove_dir_all(&root);
            return Err("service did not bind start to the installed state".to_owned());
        }
        let text = std::fs::read_to_string(&plist_path).map_err(|error| error.to_string())?;
        if !text.contains("<string>/opt/velnor-host</string>\n<string>--state</string><string>")
            || !text.contains("<string>daemon</string>\n<string>run</string>")
        {
            let _ = std::fs::remove_dir_all(&root);
            return Err("LaunchAgent arguments are not exact".to_owned());
        }
        std::fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn plist_install_does_not_follow_a_symlink() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("velnor-service-link-{}", std::process::id()));
        let state = root.join("state");
        let logs = root.join("logs");
        let target = root.join("target.plist");
        let path = root.join("agent.plist");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        std::fs::write(&target, b"keep this target").map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(&target, &path).map_err(|error| error.to_string())?;
        let written = write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs);
        let target_text = std::fs::read(&target).map_err(|error| error.to_string())?;
        let path_is_link = std::fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink();
        std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        if written || target_text != b"keep this target" || !path_is_link {
            return Err("plist install followed a symlink".to_owned());
        }
        Ok(())
    }

    #[test]
    fn uninstall_keeps_the_plist_when_launchctl_stop_fails() -> Result<(), String> {
        let path = std::env::temp_dir().join(format!(
            "velnor-uninstall-{}-{}.plist",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(&path, b"owned plist").map_err(|error| error.to_string())?;
        let result = uninstall_after_stop(ExitCode::from(1), &path);
        let exists = path.is_file();
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        if result != ExitCode::from(1) || !exists {
            return Err("uninstall hid a failed stop".to_owned());
        }
        Ok(())
    }
}
