//! Fixed step templates over validated command strings.
//!
//! Templates fix names, env keys, and payload shapes; argv arrives validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, commands};

/// Env key selecting the staged-binary internal operation.
pub const INTERNAL_OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Env key carrying the JSON request-file path.
pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
/// Planner operation name.
pub const PLAN_OPERATION: &str = "plan-v1";
/// Report-merge operation name.
pub const MERGE_OPERATION: &str = "merge-v1";
/// Required prefix of the digest-verified staged binary path.
pub const STAGED_BINARY_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";
/// Required prefix of internal request directories.
pub const REQUEST_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/";
/// Env key carrying the downloaded asset SHA-256.
pub const ASSET_SHA_ENV: &str = "VELNOR_ASSET_SHA256";
/// Env key carrying the downloaded asset URL.
pub const ASSET_URL_ENV: &str = "VELNOR_ASSET_URL";
/// Pinned mr-boxington action name (objects mode).
pub const MBX_ACTION_NAME: &str = "jdx/mr-boxington-action";
/// Cache restore/save action names.
pub const CACHE_RESTORE_NAME: &str = "actions/cache/restore";
/// Cache save action name.
pub const CACHE_SAVE_NAME: &str = "actions/cache/save";
/// Task-artifacts dir archived for task-result reuse.
pub const TASK_ARTIFACTS_DIR: &str = "$MISE_TASK_CACHE_DIR/task-artifacts/v2";
/// Target-directory prefix isolating one lane.
pub const TARGET_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/target/";
/// Substrings that must never appear in rendered YAML.
pub const FORBIDDEN_TOKENS: &[&str] = &["__internal", "velnor-actions __", "velnor-actions run"];

/// Reject text containing a private-subcommand or parallel token.
/// # Errors
pub fn scan_for_private_subcommands(text: &str) -> Result<(), RenderError> {
    for token in FORBIDDEN_TOKENS {
        if text.contains(token) {
            return Err(RenderError::PrivateSubcommand((*token).to_owned()));
        }
    }
    Ok(())
}

/// Validate an `owner/repo@<40 hex>` action ref; branches are rejected.
/// # Errors
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    let Some((name, sha)) = uses.split_once('@') else {
        return Err(RenderError::BadActionRef(format!("missing_sha:{uses}")));
    };
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    };
    if owner.is_empty() || repo.is_empty() || !is_action_name(name) {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    }
    if name.starts_with("actions/setup-") || name == "taiki-e/install-action" {
        return Err(RenderError::BadActionRef(format!(
            "forbidden_action:{uses}"
        )));
    }
    if sha.len() != 40 || !is_lower_hex(sha) {
        return Err(RenderError::BadActionRef(format!("unpinned_ref:{uses}")));
    }
    Ok(())
}

/// Checkout step without persisted credentials.
/// # Errors
pub fn checkout_step(uses: &str) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!("not_checkout:{uses}")));
    }
    let with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    Ok(Step {
        name: "Checkout".to_owned(),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
        },
    })
}

/// Validated pinned-action step.
/// # Errors
pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_name".to_owned()));
    }
    validate_uses(uses)?;
    scan_for_private_subcommands(name)?;
    scan_for_private_subcommands(uses)?;
    for entry in with.iter().flat_map(|(key, value)| [key, value]) {
        scan_for_private_subcommands(entry)?;
    }
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
        },
    })
}

/// Validated fixed-argv shell step.
/// # Errors
pub fn shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Shell { run: argv, env },
    })
}

/// Acquire-Velnor step: digest-verified staging under runner temp.
/// # Errors
pub fn acquire_velnor_step(
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    if env
        .get(ASSET_SHA_ENV)
        .is_none_or(|sha| sha.len() != 64 || !is_lower_hex(sha))
    {
        return Err(RenderError::BadCommand("bad_asset_sha256".to_owned()));
    }
    if env
        .get(ASSET_URL_ENV)
        .is_none_or(|url| !url.starts_with("https://"))
    {
        return Err(RenderError::BadCommand("bad_asset_url".to_owned()));
    }
    if !argv.iter().any(|arg| arg.contains(STAGED_BINARY_PREFIX)) {
        return Err(RenderError::BadCommand("unstaged_binary".to_owned()));
    }
    shell_step("Acquire Velnor", argv, env)
}

/// Internal plan/merge step; operation travels via env, never argv.
/// # Errors
pub fn internal_step(name: &str, operation: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    if operation != PLAN_OPERATION && operation != MERGE_OPERATION {
        return Err(RenderError::BadCommand(format!(
            "unknown_internal_op:{operation}"
        )));
    }
    scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Internal {
            operation: operation.to_owned(),
        },
    })
}

/// Fixed planner step (`plan-v1`).
#[must_use]
pub fn plan_step() -> Step {
    Step {
        name: "Plan".to_owned(),
        kind: StepKind::Internal {
            operation: PLAN_OPERATION.to_owned(),
        },
    }
}

/// Fixed report-merge step (`merge-v1`).
#[must_use]
pub fn merge_step() -> Step {
    Step {
        name: "Merge reports".to_owned(),
        kind: StepKind::Internal {
            operation: MERGE_OPERATION.to_owned(),
        },
    }
}

/// Objects-mode MBX step; cargo profiles must never emit or install MBX.
/// # Errors
pub fn mbx_objects_step(uses: &str, cargo_profile: bool) -> Result<Step, RenderError> {
    if cargo_profile {
        return Err(RenderError::BadCommand("cargo_profile_no_mbx".to_owned()));
    }
    validate_uses(uses)?;
    if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
        return Err(RenderError::BadActionRef(format!("not_mbx_action:{uses}")));
    }
    let with = BTreeMap::from([("mode".to_owned(), "objects".to_owned())]);
    action_step("Restore MBX objects", uses, with)
}

/// Cache restore/save step over `actions/cache`; MBX never archives here.
/// # Errors
pub fn cache_action_step(
    restore: bool,
    uses: &str,
    layer: &str,
    key: &str,
    restore_keys: &[String],
    paths: &[String],
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    let want = if restore {
        CACHE_RESTORE_NAME
    } else {
        CACHE_SAVE_NAME
    };
    if !uses.starts_with(&format!("{want}@")) {
        return Err(RenderError::BadActionRef(format!("bad_cache_uses:{uses}")));
    }
    if !matches!(layer, "sources" | "task") {
        return Err(RenderError::BadCommand("mbx_needs_objects_mode".to_owned()));
    }
    if key.trim().is_empty() || key.contains(' ') || key.contains('\n') {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
    }
    for path in paths {
        validate_cache_path(layer, path)?;
    }
    let mut with = BTreeMap::from([
        ("key".to_owned(), key.to_owned()),
        ("path".to_owned(), paths.join("\n")),
    ]);
    if restore {
        with.insert("restore-keys".to_owned(), restore_keys.join("\n"));
    }
    let name = if restore {
        "Restore cache"
    } else {
        "Save cache"
    };
    action_step(name, uses, with)
}

fn validate_cache_path(layer: &str, path: &str) -> Result<(), RenderError> {
    let second = path.split('/').nth(1);
    let sources_ok = path.starts_with("$CARGO_HOME/") && matches!(second, Some("registry" | "git"));
    if layer == "sources" && sources_ok || layer == "task" && path == TASK_ARTIFACTS_DIR {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_cache_path:{path}")))
    }
}

/// Isolated target directory for one lane.
#[must_use]
pub fn target_dir_for_lane(lane_id: &str) -> String {
    format!("{TARGET_DIR_PREFIX}{lane_id}")
}

/// Double-quote an env-derived shell path for `run:` lines.
#[must_use]
pub fn quote_env_path_for_run(path: &str) -> String {
    let quoted = path.len() >= 2 && path.starts_with('"') && path.ends_with('"');
    if quoted || !has_shell_expansion(path) {
        return path.to_owned();
    }
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

/// True when a `run:` line carries a bare `$VAR/` word-splitting pattern.
#[must_use]
pub fn has_bare_env_expansion(line: &str) -> bool {
    scan_run_words(line).iter().any(|(_, bare)| *bare)
}

/// Quote bare env words in a joined `run:` line (idempotent).
#[must_use]
pub fn quote_run_line_env_paths(run_line: &str) -> String {
    scan_run_words(run_line)
        .into_iter()
        .map(|(word, bare)| {
            if bare {
                quote_env_path_for_run(&decode_run_word(&word))
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn scan_run_words(run_line: &str) -> Vec<(String, bool)> {
    let bytes = run_line.as_bytes();
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut bare = false;
    let mut quote = 0u8;
    let mut chars = run_line.char_indices();
    while let Some((index, ch)) = chars.next() {
        if quote == 1 {
            cur.push(ch);
            if ch == '\'' {
                quote = 0;
            }
        } else if quote == 2 {
            cur.push(ch);
            if ch == '\\' {
                cur.extend(chars.next().map(|(_, next)| next));
            } else if ch == '"' {
                quote = 0;
            }
        } else if ch == '\'' {
            quote = 1;
            cur.push(ch);
        } else if ch == '"' {
            quote = 2;
            cur.push(ch);
        } else if ch.is_whitespace() {
            if !cur.is_empty() {
                words.push((std::mem::take(&mut cur), std::mem::take(&mut bare)));
            }
        } else {
            bare = bare || ch == '$' && is_shell_expansion_at(bytes, index);
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        words.push((cur, bare));
    }
    words
}

/// Decode single-quote spans (`'\''` → `'`).
fn decode_run_word(word: &str) -> String {
    word.replace("'\\''", "\0")
        .replace('\'', "")
        .replace('\0', "'")
}

/// True for `owner/repo` over alphanumerics plus `.-_`.
fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}

/// True for 64-char lowercase hex.
fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// True when text holds a `$NAME`/`${NAME}` shell expansion span.
fn has_shell_expansion(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'$' && is_shell_expansion_at(bytes, index))
}

/// True when `bytes[index] == b'$'` starts a shell span (`${{` excluded).
fn is_shell_expansion_at(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index + 1) {
        Some(b'{') => bytes.get(index + 2) != Some(&b'{'),
        Some(next) => next.is_ascii_alphabetic() || *next == b'_',
        None => false,
    }
}
