use std::path::{Path, PathBuf};

use velnor_runner_host::launch_agent_plist;

#[cfg(test)]
use super::filesystem::regular_file_or_absent;
#[cfg(test)]
use super::quarantine::existing_matches;

use super::LABEL;

pub(super) fn render_plist(binary: &Path, state: &Path, logs: &Path) -> Option<String> {
    let binary = binary
        .to_str()
        .filter(|text| !text.chars().any(xml_forbidden))?;
    let binary = Path::new(binary);
    let state = std::fs::canonicalize(state).ok()?;
    let plist = launch_agent_plist(binary).ok()?;
    let plist = with_state(&plist, &state)?;
    with_logs(&plist, logs)
}

#[cfg(test)]
pub(super) fn plist_matches(path: &Path, binary: &Path, state: &Path, logs: &Path) -> bool {
    if !regular_file_or_absent(path) {
        return false;
    }
    let Some(expected) = render_plist(binary, state, logs) else {
        return false;
    };
    existing_matches(path, expected.as_bytes()).unwrap_or(false)
}

pub(super) fn with_state(plist: &str, state: &Path) -> Option<String> {
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

pub(super) fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

pub(super) fn with_logs(plist: &str, log_dir: &Path) -> Option<String> {
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
