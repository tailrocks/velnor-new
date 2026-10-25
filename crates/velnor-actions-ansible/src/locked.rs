use crate::AnsibleError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockedTool {
    pub key: &'static str,

    pub url_prefix: &'static str,
}

pub const UV: LockedTool = LockedTool {
    key: "uv",
    url_prefix: "https://github.com/astral-sh/uv/releases/download/",
};

pub const PYTHON: LockedTool = LockedTool {
    key: "python",
    url_prefix: "https://github.com/astral-sh/python-build-standalone/releases/download/",
};

pub const PLATFORM: &str = "linux-x64";

#[must_use]
pub fn check_locked_tool(lock_text: &str, tool: LockedTool) -> Vec<AnsibleError> {
    let Ok(lock) = lock_text.parse::<toml::Table>() else {
        return vec![AnsibleError::bare("ansible_tool_lock_unparsed")];
    };
    let entry = lock
        .get("tools")
        .and_then(|tools| tools.get(tool.key))
        .and_then(toml::Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find_map(|row| row.get(format!("platforms.{PLATFORM}")))
        });
    let Some(entry) = entry else {
        return vec![AnsibleError::new("ansible_tool_lock_missing", tool.key)];
    };
    let mut findings = Vec::new();
    let url = entry.get("url").and_then(toml::Value::as_str);
    if !url.is_some_and(|url| url.starts_with(tool.url_prefix)) {
        findings.push(AnsibleError::new("ansible_tool_url_untrusted", tool.key));
    }
    let checksum = entry.get("checksum").and_then(toml::Value::as_str);
    let hex = checksum.and_then(|text| text.strip_prefix("sha256:"));
    if !hex.is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }) {
        findings.push(AnsibleError::new("ansible_tool_checksum_missing", tool.key));
    }
    findings
}
