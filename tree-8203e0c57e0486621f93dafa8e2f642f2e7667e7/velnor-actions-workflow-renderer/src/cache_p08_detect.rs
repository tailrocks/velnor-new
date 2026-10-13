//! Bounded shell-word detection for Mise cache qualification.
//!
//! Workflow IR keeps fixed argv but no Mise metadata. This module detects
//! Mise tools from direct command heads and one direct Mise payload. It does
//! not evaluate repository code or general shell state.

use crate::commands::is_inline_shell;

#[path = "cache_p08_detect_lex_heredoc.rs"]
mod heredoc;
#[path = "cache_p08_detect_lex.rs"]
mod lex;

/// One detected shell command after redirection removal.
pub(crate) struct DetectedCommand {
    /// Shell words for one command.
    pub words: Vec<String>,
}

/// Parsed commands and whether the fixed command grammar was exceeded.
pub(crate) struct Detection {
    /// Parsed commands in source order.
    pub commands: Vec<DetectedCommand>,
    /// Unsupported syntax or executable expansion cannot yield a cache key.
    pub unsupported_mise_syntax: bool,
}

/// Parse one fixed argv or supported inline-shell script.
pub(crate) fn detect_commands(run: &[String]) -> Detection {
    if is_inline_shell(run) {
        return lex::detect_script(&run[2], 0);
    }
    let words = executable_words(run);
    if is_inline_shell(&words) {
        return lex::detect_script(&words[2], 0);
    }
    let unsupported_mise_syntax = command_has_unmodeled_execution_head(&words);
    let command = DetectedCommand { words };
    Detection {
        commands: vec![command],
        unsupported_mise_syntax,
    }
}

/// True when one parsed command starts Mise after supported wrappers.
pub(crate) fn command_starts_mise(words: &[String]) -> bool {
    executable_words(words)
        .first()
        .is_some_and(|word| is_mise_executable(word))
}

/// True when a command head needs classification outside the direct Mise parser.
pub(crate) fn command_has_unmodeled_execution_head(words: &[String]) -> bool {
    let starts_mise = command_starts_mise(words);
    command_has_dynamic_executable(words)
        || command_has_unsupported_launcher(words)
        // Direct Mise heads own their bounded `exec --` payload parsing.
        || (!starts_mise && command_has_unmodeled_mise_payload(words))
}

/// True when the executable is expanded at run time.
pub(crate) fn command_has_dynamic_executable(words: &[String]) -> bool {
    executable_words(words)
        .first()
        .is_some_and(|word| word.contains('$') || word.contains('`'))
}

/// True when the command head uses a launcher outside the supported argv subset.
pub(crate) fn command_has_unsupported_launcher(words: &[String]) -> bool {
    let Some(first) = words.first().map(String::as_str) else {
        return false;
    };
    matches!(
        first,
        "sudo"
            | "doas"
            | "nice"
            | "nohup"
            | "timeout"
            | "setsid"
            | "stdbuf"
            | "xargs"
            | "eval"
            | "busybox"
            | "pkexec"
            | "strace"
            | "ionice"
            | "chronic"
            | "flock"
            | "systemd-run"
            | "time"
            | "sh"
            | "bash"
            | "!"
    ) || first == "exec"
        && words
            .get(1)
            .is_some_and(|argument| argument.starts_with('-') && argument != "--")
        || first == "env"
            && executable_words(words)
                .first()
                .is_some_and(|executable| executable == "env")
        || first == "command"
            && words.get(1).is_some_and(|argument| {
                argument.starts_with('-') && !matches!(argument.as_str(), "-p" | "-v" | "-V" | "--")
            })
}

/// True when an opaque command passes a direct Mise invocation as an argument.
pub(crate) fn command_has_unmodeled_mise_payload(words: &[String]) -> bool {
    let argv = executable_words(words);
    if argv
        .first()
        .is_some_and(|head| matches!(head.as_str(), "echo" | "printf"))
    {
        return false;
    }
    argv.windows(2)
        .skip(1)
        .any(|pair| is_mise_executable(&pair[0]) && matches!(pair[1].as_str(), "install" | "exec"))
}

/// Tool arguments of a supported `mise install` or `mise exec` command.
pub(crate) fn mise_tool_candidates(words: &[String]) -> Vec<String> {
    mise_tool_candidates_at(words, false)
}

fn mise_tool_candidates_at(words: &[String], nested: bool) -> Vec<String> {
    let argv = executable_words(words);
    if argv.first().is_some_and(|word| is_path_mise(word)) {
        return vec!["mise@unsupported-executable-path".to_owned()];
    }
    if argv.first().is_none_or(|word| word != "mise") {
        return Vec::new();
    }
    let Some(subcommand) = mise_subcommand(&argv) else {
        return if argv
            .iter()
            .any(|word| matches!(word.as_str(), "--help" | "--version" | "-h"))
        {
            Vec::new()
        } else {
            vec!["mise@unsupported-command".to_owned()]
        };
    };
    let is_exec = argv[subcommand] == "exec";
    let mut tools = Vec::new();
    let mut payload = None;
    for (offset, word) in argv[subcommand + 1..].iter().enumerate() {
        if is_exec && word == "--" {
            payload = Some(&argv[subcommand + 2 + offset..]);
            break;
        }
        if matches!(word.as_str(), "--help" | "-h") {
            return Vec::new();
        }
        if is_global_mise_flag(word) {
            continue;
        }
        if word.starts_with('-') {
            return vec!["mise@unsupported-options".to_owned()];
        }
        if !word.starts_with('-') {
            // Preserve unpinned, malformed, and moving positional specs so
            // cache-key validation rejects them instead of omitting tools.
            tools.push(word.clone());
        }
    }
    if tools.is_empty() {
        tools.push("mise@missing-exact-tool".to_owned());
    }
    if let Some(payload) = payload {
        if nested
            || payload.is_empty()
            || command_has_dynamic_executable(payload)
            || command_has_unsupported_launcher(payload)
            || command_has_unmodeled_mise_payload(payload)
        {
            tools.push("mise@unsupported-exec-payload".to_owned());
        } else if command_starts_mise(payload) {
            tools.extend(mise_tool_candidates_at(payload, true));
        }
    }
    tools
}

fn mise_subcommand(argv: &[String]) -> Option<usize> {
    for (index, word) in argv.iter().enumerate().skip(1) {
        if is_global_mise_flag(word) {
            continue;
        }
        if matches!(word.as_str(), "install" | "exec") {
            return Some(index);
        }
        if word.starts_with('-') {
            return None;
        }
        return None;
    }
    None
}

fn is_global_mise_flag(word: &str) -> bool {
    matches!(
        word,
        "--no-config"
            | "--no-env"
            | "--no-hooks"
            | "--yes"
            | "-y"
            | "--quiet"
            | "-q"
            | "--verbose"
            | "-v"
    )
}

fn is_mise_executable(word: &str) -> bool {
    word == "mise" || is_path_mise(word)
}

fn is_path_mise(word: &str) -> bool {
    let basename = word.rsplit(['/', '\\']).next().unwrap_or(word);
    basename == "mise" && word != "mise"
}

/// Return shell argv after the supported transparent command wrappers.
fn executable_words(words: &[String]) -> Vec<String> {
    let mut cursor = 0;
    let mut remaining = words;
    while let Some(word) = remaining.get(cursor) {
        if is_env_assignment(word) {
            cursor += 1;
            continue;
        }
        if word == "time" && remaining.get(cursor + 1).is_some_and(|next| next == "mise") {
            cursor += 1;
            continue;
        }
        if word == "!" || word == "command" || word == "builtin" || word == "exec" {
            if word == "exec"
                && remaining
                    .get(cursor + 1)
                    .is_some_and(|argument| argument.starts_with('-') && argument != "--")
            {
                break;
            }
            if word == "command" && remaining.get(cursor + 1).is_some_and(|arg| arg == "-p") {
                cursor += 2;
                continue;
            }
            if word == "command"
                && remaining
                    .get(cursor + 1)
                    .is_some_and(|arg| matches!(arg.as_str(), "-v" | "-V"))
            {
                break;
            }
            if word == "command"
                && remaining
                    .get(cursor + 1)
                    .is_some_and(|argument| argument.starts_with('-') && argument != "--")
            {
                break;
            }
            cursor += 1;
            if remaining.get(cursor).is_some_and(|arg| arg == "--") {
                cursor += 1;
            }
            continue;
        }
        if word == "env" {
            let env_start = cursor;
            cursor += 1;
            while cursor < remaining.len() {
                match remaining[cursor].as_str() {
                    "-u" | "--unset" if cursor + 1 < remaining.len() => cursor += 2,
                    "-i" | "--ignore-environment" | "-0" | "--null" => cursor += 1,
                    value if value.starts_with("--unset=") => cursor += 1,
                    "-C" | "--chdir" if cursor + 1 < remaining.len() => cursor += 2,
                    value if value.starts_with("--chdir=") => cursor += 1,
                    "--" => {
                        cursor += 1;
                        break;
                    }
                    value if is_env_assignment(value) => cursor += 1,
                    value if value.starts_with('-') => return remaining[env_start..].to_vec(),
                    _ => break,
                }
            }
            continue;
        }
        break;
    }
    remaining = &remaining[cursor..];
    remaining.to_vec()
}

fn is_env_assignment(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(key, _)| !key.is_empty() && key.bytes().all(is_env_name_byte))
}

fn is_env_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
