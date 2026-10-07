//! Narrow recognition of actual MBX command positions in rendered steps.

use velnor_actions_contract_workflow::{Step, StepKind};

const MISE_PREFIX: [&str; 5] = ["mise", "--no-config", "--no-env", "--no-hooks", "exec"];
const REPORT_WRAPPER_PREFIX: &str = "s=$(date +%s%3N); ";
const REPORT_WRAPPER_SEPARATOR: &str = "; code=$?; ";
const REPORT_WRAPPER_SUFFIX: &str =
    "VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$s\" VELNOR_INTERNAL_OP=write-task-report-v1 ";
const MISE_INSTALL_PREFIX: [&str; 4] = ["mise", "--no-config", "--no-env", "--no-hooks"];

#[derive(Debug, PartialEq, Eq)]
enum ShellToken {
    Word(String),
    Operator(String),
    ReservedIf,
    ReservedBang,
}

impl ShellToken {
    fn word(&self) -> Option<&str> {
        match self {
            Self::Word(word) => Some(word),
            Self::Operator(_) | Self::ReservedIf | Self::ReservedBang => None,
        }
    }

    fn is_separator(&self) -> bool {
        matches!(self, Self::Operator(_))
    }
}

/// True when one supported Mise command selects MBX outside the native action.
pub(super) fn has_external_mbx_selector(step: &Step) -> bool {
    match &step.kind {
        StepKind::Shell { run, .. } => {
            let payload =
                &run[velnor_actions_workflow_steps::toolchain_env::unset_prefix_len(run)..];
            let Some(words) = selector_words(payload) else {
                return true;
            };
            has_mbx_selector(&words)
        }
        StepKind::Action { uses, with, .. }
            if uses.starts_with(&format!(
                "{}@",
                velnor_actions_workflow_steps::setup::MISE_ACTION_NAME
            )) =>
        {
            let setup_installs_tools = with.get("install").is_none_or(|install| install != "false");
            setup_installs_tools
                || with
                    .get("tool_versions")
                    .is_some_and(|specs| specs.split_whitespace().any(is_mbx_selector))
        }
        _ => false,
    }
}

fn selector_words(run: &[String]) -> Option<Vec<ShellToken>> {
    if velnor_actions_workflow_steps::commands::is_inline_shell(run) {
        shell_words(run.get(2)?)
    } else {
        Some(run.iter().cloned().map(ShellToken::Word).collect())
    }
}

/// Split supported shell words and unquoted operators without erasing token identity.
fn shell_words(script: &str) -> Option<Vec<ShellToken>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    let mut reserved = true;
    for character in script.chars() {
        if escaped {
            if character != '\n' {
                current.push(character);
            }
            escaped = false;
            started = true;
            reserved = false;
            continue;
        }
        if let Some(delimiter) = quote {
            match character {
                value if value == delimiter => quote = None,
                '\\' if delimiter == '"' => escaped = true,
                value => current.push(value),
            }
            started = true;
            continue;
        }
        match character {
            '\\' => {
                escaped = true;
                started = true;
                reserved = false;
            }
            '\'' | '"' => {
                quote = Some(character);
                started = true;
                reserved = false;
            }
            ' ' | '\t' | '\r' => {
                push_shell_word(&mut words, &mut current, &mut started, reserved);
                reserved = true;
            }
            '\n' | ';' | '&' | '|' | '(' | ')' | '{' | '}' => {
                push_shell_word(&mut words, &mut current, &mut started, reserved);
                words.push(ShellToken::Operator(character.to_string()));
                reserved = true;
            }
            value => {
                current.push(value);
                started = true;
            }
        }
    }
    if escaped || quote.is_some() {
        return None;
    }
    push_shell_word(&mut words, &mut current, &mut started, reserved);
    Some(words)
}

fn push_shell_word(
    words: &mut Vec<ShellToken>,
    current: &mut String,
    started: &mut bool,
    reserved: bool,
) {
    if *started {
        let word = std::mem::take(current);
        words.push(if reserved && word == "if" {
            ShellToken::ReservedIf
        } else if reserved && word == "!" {
            ShellToken::ReservedBang
        } else {
            ShellToken::Word(word)
        });
        *started = false;
    }
}

fn has_mbx_selector(words: &[ShellToken]) -> bool {
    (0..words.len()).any(|start| {
        words[start].word() == Some("mise")
            && is_command_start(words, start)
            && has_mbx_selector_in_command(words, start)
    })
}

fn has_mbx_selector_in_command(words: &[ShellToken], start: usize) -> bool {
    let prefix_matches = words
        .get(start..start + MISE_INSTALL_PREFIX.len())
        .is_some_and(|prefix| {
            prefix
                .iter()
                .map(ShellToken::word)
                .eq(MISE_INSTALL_PREFIX.iter().copied().map(Some))
        });
    if !prefix_matches {
        return false;
    }
    let Some(subcommand) = words.get(start + MISE_INSTALL_PREFIX.len()) else {
        return false;
    };
    let first_selector = start + MISE_INSTALL_PREFIX.len() + 1;
    match subcommand.word() {
        Some("install") => words[first_selector..]
            .iter()
            .take_while(|word| !word.is_separator())
            .filter_map(ShellToken::word)
            .any(is_mbx_selector),
        Some("exec") => words[first_selector..]
            .iter()
            .take_while(|word| word.word() != Some("--") && !word.is_separator())
            .filter_map(ShellToken::word)
            .any(is_mbx_selector),
        _ => false,
    }
}

fn is_command_start(words: &[ShellToken], index: usize) -> bool {
    let mut command_start = index;
    while command_start > 0 && is_command_prefix(&words[command_start - 1]) {
        command_start -= 1;
    }
    command_start == 0
        || words
            .get(command_start - 1)
            .is_some_and(ShellToken::is_separator)
}

fn is_command_prefix(word: &ShellToken) -> bool {
    match word {
        ShellToken::ReservedIf | ShellToken::ReservedBang => true,
        ShellToken::Word(word) => {
            matches!(word.as_str(), "command" | "builtin" | "env")
                || word
                    .split_once('=')
                    .is_some_and(|(name, _)| is_shell_identifier(name))
        }
        ShellToken::Operator(_) => false,
    }
}

fn is_shell_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_mbx_selector(word: &str) -> bool {
    let selector = word.trim_matches(|character| {
        matches!(character, '\'' | '"' | ',' | ';' | '&' | '|' | ')' | '(')
    });
    selector == "mr-boxington" || selector.starts_with("mr-boxington@")
}

pub(super) fn uses_mbx_command(step: &Step) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    let payload = &run[velnor_actions_workflow_steps::toolchain_env::unset_prefix_len(run)..];
    if is_mbx_argv(payload) {
        return true;
    }
    let [shell, flag, script] = payload else {
        return false;
    };
    if shell != "sh" || flag != "-c" {
        return false;
    }
    if let Some(command) = report_wrapped_command(script) {
        return command_line_uses_mbx(command);
    }
    command_line_uses_mbx(script)
}

fn report_wrapped_command(script: &str) -> Option<&str> {
    let script = script
        .strip_prefix(&velnor_actions_workflow_steps::toolchain_env::credential_unset_prelude())?
        .strip_prefix(' ')?;
    let body = script.strip_prefix(REPORT_WRAPPER_PREFIX)?;
    let (command, suffix) = body.split_once(REPORT_WRAPPER_SEPARATOR)?;
    suffix.starts_with(REPORT_WRAPPER_SUFFIX).then_some(command)
}

fn command_line_uses_mbx(command: &str) -> bool {
    let argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    is_mbx_argv(&argv)
}

fn is_mbx_argv(argv: &[String]) -> bool {
    if argv.first().is_some_and(|program| program == "mbx") {
        return true;
    }
    if argv.len() <= MISE_PREFIX.len() || !has_prefix(argv, &MISE_PREFIX) {
        return false;
    }
    let Some(separator) = argv.iter().position(|arg| arg == "--") else {
        return false;
    };
    separator > MISE_PREFIX.len()
        && argv[MISE_PREFIX.len()..separator]
            .first()
            .is_some_and(|spec| is_exact_rust_spec(spec))
        && argv
            .get(separator + 1)
            .is_some_and(|program| program == "mbx")
}

fn has_prefix(argv: &[String], prefix: &[&str]) -> bool {
    argv.iter()
        .take(prefix.len())
        .map(String::as_str)
        .eq(prefix.iter().copied())
}

fn is_exact_rust_spec(spec: &str) -> bool {
    let Some(version) = spec.strip_prefix("rust@") else {
        return false;
    };
    let mut parts = version.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if [major, minor, patch].iter().all(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
            })
    )
}
