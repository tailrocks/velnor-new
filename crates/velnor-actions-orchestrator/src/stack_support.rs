//! Helpers shared by the dbt and Ansible stacks: the Mise-declared version of a
//! tool, the lock text, and the checksum-verified archive install script.

use std::path::Path;

use velnor_actions_contract::FileIndex;
use velnor_actions_contract::config::project_tool_refs::{
    PROJECT_MISE_CONFIG_PATTERNS, PROJECT_TOOL_LOCK_FILES, matches_source_pattern,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Read one repository text file: absent, or text within the bound.
pub(crate) fn read_text(root: &Path, rel: &str) -> Result<Option<String>, OrchestratorError> {
    match read_repo_file(root, rel, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(text) => Ok(Some(text)),
        RepoRead::Absent => Ok(None),
    }
}

/// The text of the repository `mise.lock`, when one exists.
pub(crate) fn read_lock(root: &Path) -> Result<Option<String>, OrchestratorError> {
    for name in PROJECT_TOOL_LOCK_FILES {
        if let Some(text) = read_text(root, name)? {
            return Ok(Some(text));
        }
    }
    Ok(None)
}

/// The one version every Mise config source declares under any of `keys`.
///
/// `stack` names the failing table (`stacks.dbt`). `None` when no source
/// declares the tool; two sources that disagree fail closed.
pub(crate) fn declared_version(
    root: &Path,
    index: &FileIndex,
    keys: &[&str],
    stack: &str,
) -> Result<Option<String>, OrchestratorError> {
    let error = |problem: String| OrchestratorError::config(CONFIG_REL, stack, problem);
    let mut declared: Option<String> = None;
    for file in index.files().iter().filter(|file| {
        PROJECT_MISE_CONFIG_PATTERNS
            .iter()
            .any(|pattern| matches_source_pattern(pattern, file))
    }) {
        let Some(text) = read_text(root, file)? else {
            continue;
        };
        let value: toml::Value =
            toml::from_str(&text).map_err(|_| error(format!("mise_config_not_toml:{file}")))?;
        let Some(tools) = value.get("tools").and_then(toml::Value::as_table) else {
            continue;
        };
        for key in keys {
            let version = match tools.get(*key) {
                Some(toml::Value::String(text)) => Some(text.clone()),
                Some(toml::Value::Table(table)) => table
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                _ => None,
            };
            let Some(version) = version else { continue };
            match &declared {
                Some(first) if *first != version => {
                    return Err(error(format!(
                        "conflicting_project_tool_versions:{key},{file}"
                    )));
                }
                Some(_) => {}
                None => declared = Some(version),
            }
        }
    }
    Ok(declared)
}

/// `bash -c <script>` argv.
pub(crate) fn bash(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// Single-quote a validated repository path or literal for the shell.
pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The awk program that prints `<url> <sha256>` of the declared version's row.
///
/// `table` is the `[tools.<key>."platforms.<p>"]` header, `want` the version,
/// and `key` the tool key of the `[[tools.<key>]]` array header.
const ROW_PROGRAM: &str = "$0 == arr { ver = \"\"; row = 0; next } \
/^\\[/ { row = ($0 == table && ver == want); next } \
/^version = \"/ && !row { ver = $0; sub(/^version = \"/, \"\", ver); sub(/\"$/, \"\", ver); next } \
row && /^url = \"/ { url = $0; sub(/^url = \"/, \"\", url); sub(/\"$/, \"\", url) } \
row && /^checksum = \"sha256:/ { sum = $0; sub(/^checksum = \"sha256:/, \"\", sum); sub(/\"$/, \"\", sum) } \
END { print url, sum }";

/// One archive installed from the `linux-x64` row of `mise.lock`.
pub(crate) struct LockedArchive<'a> {
    /// Key under `[tools]` in the lock (`dbt-fusion`, `uv`, `python`).
    pub(crate) key: &'a str,
    /// Project-tool reference variable (`VELNOR_TOOL_UV`).
    pub(crate) version_var: &'a str,
    /// Run-time directory the archive extracts into (`$RUNNER_TEMP/...`).
    pub(crate) dest: &'a str,
    /// Shell that exits non-zero unless `$url` is the admitted archive URL.
    pub(crate) url_check: &'a str,
    /// Extra `tar` arguments (`--strip-components=1`), possibly empty.
    pub(crate) tar_args: &'a str,
    /// Path (under `dest`) that must be executable afterwards.
    pub(crate) binary: &'a str,
}

impl LockedArchive<'_> {
    /// The install script: select the row, check URL and checksum, download,
    /// verify, extract.
    pub(crate) fn script(&self) -> String {
        let table = format!("[tools.{}.\"platforms.linux-x64\"]", self.key);
        let array = format!("[[tools.{}]]", self.key);
        let Self {
            key,
            version_var,
            dest,
            url_check,
            tar_args,
            binary,
        } = self;
        format!(
            "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; d=\"{dest}\"; rm -rf \"$d\"; mkdir -p \"$d\"; \
lock=mise.lock; if [ ! -f \"$lock\" ]; then lock=.mise.lock; fi; test -f \"$lock\"; test ! -L \"$lock\"; \
awk -v arr={array} -v table={table} -v want=\"${{{version_var}}}\" {program} \"$lock\" > \"$d/row\"; \
IFS=' ' read -r url sum < \"$d/row\" || true; \
{url_check}; \
case \"${{sum:-}}\" in ''|*[!0-9a-f]*) echo 'velnor: the mise.lock {key} row has no sha256 checksum' >&2; exit 1 ;; esac; \
if [ \"${{#sum}}\" -ne 64 ]; then echo 'velnor: the mise.lock {key} checksum is malformed' >&2; exit 1; fi; \
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -o \"$d/archive\" \"$url\"; \
printf '%s  %s\\n' \"$sum\" \"$d/archive\" > \"$d/archive.sha256\"; sha256sum -c \"$d/archive.sha256\"; \
tar -xzf \"$d/archive\" -C \"$d\" {tar_args}; rm -f \"$d/archive\"; test -x \"$d/{binary}\"",
            array = quote(&array),
            table = quote(&table),
            program = quote(ROW_PROGRAM),
        )
    }
}
