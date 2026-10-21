use velnor_actions_ansible::check_locked_tool;
use velnor_actions_ansible::locked::{PYTHON, UV};

const SUM: &str = "eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220";

fn lock(key: &str, url: &str, checksum: Option<&str>) -> String {
    let sum = checksum.map_or_else(String::new, |sum| format!("checksum = \"{sum}\"\n"));
    format!(
        "[[tools.{key}]]\nversion = \"1.0.0\"\n\n[tools.{key}.\"platforms.linux-x64\"]\n{sum}url = \"{url}\"\n"
    )
}

fn codes(text: &str, tool: velnor_actions_ansible::LockedTool) -> Vec<String> {
    check_locked_tool(text, tool)
        .iter()
        .map(|error| error.code().to_owned())
        .collect()
}

#[test]
fn a_checksummed_release_row_passes() {
    let text = lock(
        "uv",
        "https://github.com/astral-sh/uv/releases/download/0.13.1/uv-x86_64-unknown-linux-gnu.tar.gz",
        Some(&format!("sha256:{SUM}")),
    );
    assert_eq!(codes(&text, UV), Vec::<String>::new());
}

#[test]
fn missing_rows_checksums_and_foreign_urls_are_findings() {
    assert_eq!(codes("", PYTHON), ["ansible_tool_lock_missing:python"]);
    let text = lock("uv", "https://example.com/uv.tar.gz", Some("blake3:00"));
    assert_eq!(
        codes(&text, UV),
        [
            "ansible_tool_url_untrusted:uv",
            "ansible_tool_checksum_missing:uv"
        ]
    );
    let text = lock(
        "uv",
        "https://github.com/astral-sh/uv/releases/download/0.13.1/uv.tar.gz",
        None,
    );
    assert_eq!(codes(&text, UV), ["ansible_tool_checksum_missing:uv"]);
}
