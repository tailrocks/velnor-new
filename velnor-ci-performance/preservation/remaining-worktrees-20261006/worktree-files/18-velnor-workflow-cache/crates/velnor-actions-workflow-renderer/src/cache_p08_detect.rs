//! Bounded shell-word detection for Mise cache qualification.
//!
//! Workflow IR keeps fixed argv but no Mise metadata. This module accepts tool
//! specs only from literal `mise install`/`exec` heads and one literal Mise
//! child after `mise exec --`. It normalizes environment assignments and
//! explicit `env`, `command`, `builtin`, and `exec --` prefixes. Dynamic heads,
//! shell launchers, control-flow-wrapped Mise commands, and opaque commands
//! carrying a Mise invocation fail closed. `echo` and `printf` keep arguments
//! as data. This module does not evaluate repository code or general shell
//! state.

use crate::commands::is_inline_shell;

#[path = "cache_p08_detect_lex_heredoc.rs"]
mod heredoc;
#[path = "cache_p08_detect_lex.rs"]
mod lex;

/// One detected shell command after redirection removal.
pub(crate) struct DetectedCommand {
    /// Original shell words for one command.
    pub words: Vec<String>,
    argv: Vec<String>,
    control_prefix: bool,
}

impl DetectedCommand {
    /// Normalize one command's supported prefixes once.
    pub(crate) fn new(words: Vec<String>) -> Self {
        let control_prefix = words.first().is_some_and(|word| is_control_prefix(word));
        let command = if control_prefix { &words[1..] } else { &words };
        let argv = executable_words(command);
        Self {
            words,
            argv,
            control_prefix,
        }
    }

    /// True when the normalized executable is Mise.
    pub(crate) fn starts_mise(&self) -> bool {
        self.argv
            .first()
            .is_some_and(|word| is_mise_executable(word))
    }

    /// True when this command cannot safely contribute a cache key.
    fn has_unmodeled_execution_head(&self) -> bool {
        let starts_mise = self.starts_mise();
        self.control_prefix && starts_mise
            || command_has_dynamic_executable(&self.argv)
            || command_has_unsupported_launcher(&self.argv)
            || !starts_mise && command_has_unmodeled_mise_payload(&self.argv)
    }

    /// Exact tool specs from a normalized direct Mise command.
    pub(crate) fn tool_candidates(&self) -> Vec<String> {
        mise_tool_candidates_at(&self.argv, false)
    }
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
    let command = DetectedCommand::new(run.to_vec());
    let unsupported_mise_syntax = command.has_unmodeled_execution_head();
    Detection {
        commands: vec![command],
        unsupported_mise_syntax,
    }
}

/// True when the executable is expanded at run time.
fn command_has_dynamic_executable(argv: &[String]) -> bool {
    argv.first()
        .is_some_and(|word| word.contains('$') || word.contains('`'))
}

/// True when the command head uses a launcher outside the supported argv subset.
fn command_has_unsupported_launcher(argv: &[String]) -> bool {
    let Some(first) = argv.first().map(String::as_str) else {
        return false;
    };
    let name = executable_name(first);
    matches!(
        name,
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
            | "dash"
            | "zsh"
            | "ksh"
            | "mksh"
            | "ash"
            | "!"
    ) || name == "exec"
        && argv
            .get(1)
            .is_some_and(|argument| argument.starts_with('-') && argument != "--")
        || name == "env"
        || name == "command"
            && argv.get(1).is_some_and(|argument| {
                argument.starts_with('-') && !matches!(argument.as_str(), "-p" | "-v" | "-V" | "--")
            })
}

fn executable_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// True when an opaque command passes a direct Mise invocation as an argument.
fn command_has_unmodeled_mise_payload(argv: &[String]) -> bool {
    if argv
        .first()
        .is_some_and(|head| matches!(head.as_str(), "echo" | "printf"))
    {
        return false;
    }
    argv.iter()
        .enumerate()
        .skip(1)
        .any(|(index, word)| is_mise_executable(word) && mise_subcommand(&argv[index..]).is_some())
}

fn mise_tool_candidates_at(argv: &[String], nested: bool) -> Vec<String> {
    if argv.first().is_some_and(|word| is_path_mise(word)) {
        return vec!["mise@unsupported-executable-path".to_owned()];
    }
    if argv.first().is_none_or(|word| word != "mise") {
        return Vec::new();
    }
    let Some(subcommand) = mise_subcommand(argv) else {
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
        let child = DetectedCommand::new(payload.to_vec());
        if nested || payload.is_empty() || child.has_unmodeled_execution_head() {
            tools.push("mise@unsupported-exec-payload".to_owned());
        } else if child.starts_mise() {
            tools.extend(mise_tool_candidates_at(&child.argv, true));
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
    let basename = executable_name(word);
    basename == "mise" && word != "mise"
}

fn is_control_prefix(word: &str) -> bool {
    matches!(
        word,
        "if" | "then" | "elif" | "else" | "while" | "until" | "do" | "!"
    )
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
