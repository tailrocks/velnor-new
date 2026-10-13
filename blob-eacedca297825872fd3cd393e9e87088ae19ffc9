//! Manually validate a commit or squash message against the repository policy.
//!
//! Run with `cargo run --locked -p velnor-actions-cli --example
//! validate_commit_trailers -- MESSAGE_FILE` from the repository checkout.

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Parser;
use text_policy::{has_trailer_label, is_python_whitespace, python_lines, universal_newlines};

#[path = "support/text_policy.rs"]
mod text_policy;

const POLICY_PATH: &str = "docs/implemented/codex-agent-configuration.md";
const POLICY_HEADING: &str = "## Commit identity and trailers";
const IDENTITY_MARKER: &str = "repository-local identity `";
const COAUTHOR_LABEL: &str = "Co-authored-by";
const SIGNOFF_LABEL: &str = "Signed-off-by";

#[derive(Debug)]
struct ValidationError(String);

impl Display for ValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ValidationError {}

type CheckResult<T> = Result<T, ValidationError>;

#[derive(Debug, Eq, PartialEq)]
struct Identity {
    name: String,
    email: String,
}

#[derive(Debug)]
struct TrailerPolicy {
    identity: Identity,
    trailers: [String; 2],
}

#[derive(Debug, Parser)]
#[command(
    name = "validate-commit-trailers",
    version,
    about = "Validate a commit or squash message against the repository trailer policy"
)]
struct Args {
    /// File containing the proposed commit or squash message.
    #[arg(value_name = "MESSAGE_FILE")]
    message_file: PathBuf,
    /// Also require the effective local Git author and committer identities.
    #[arg(long = "check-local-identities")]
    check_local_identities: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match validate(&root, &args.message_file, args.check_local_identities) {
        Ok(()) => {
            println!("commit message matches the canonical trailer policy");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commit trailer validation failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn validate(root: &Path, message_file: &Path, check_local_identities: bool) -> CheckResult<()> {
    let policy_text =
        fs::read_to_string(root.join(POLICY_PATH)).map_err(|error| io_error(&error))?;
    let policy = canonical_policy(&policy_text)?;
    let message = fs::read_to_string(message_file).map_err(|error| io_error(&error))?;
    validate_message(&message, &policy.trailers)?;
    if check_local_identities {
        validate_local_identities(root, &policy.identity)?;
    }
    Ok(())
}

fn canonical_policy(text: &str) -> CheckResult<TrailerPolicy> {
    let normalized_text = universal_newlines(text);
    let text = normalized_text.as_str();
    let identity = canonical_identity(text)?;
    let heading_count = python_lines(text)
        .iter()
        .filter(|line| **line == POLICY_HEADING)
        .count();
    if heading_count != 1 {
        return Err(ValidationError(
            "canonical trailer section is missing or ambiguous".to_owned(),
        ));
    }
    let section = text
        .split_once(POLICY_HEADING)
        .map(|(_, section)| section)
        .ok_or_else(|| {
            ValidationError("canonical trailer section is missing or ambiguous".to_owned())
        })?;
    let blocks = text_fenced_blocks(section);
    if blocks.len() != 1 {
        return Err(ValidationError(
            "canonical trailer block is missing".to_owned(),
        ));
    }
    let expected: Vec<String> = blocks
        .first()
        .map(|block| {
            python_lines(block)
                .iter()
                .map(|line| (*line).to_owned())
                .collect()
        })
        .ok_or_else(|| ValidationError("canonical trailer block is missing".to_owned()))?;
    if expected.len() != 2 || expected.iter().any(String::is_empty) {
        return Err(ValidationError(
            "canonical trailer block must contain the two required lines".to_owned(),
        ));
    }
    let [coauthor, signoff]: [String; 2] = expected.try_into().map_err(|_| {
        ValidationError("canonical trailer block must contain the two required lines".to_owned())
    })?;
    if trailer_label(&coauthor) != Some(COAUTHOR_LABEL)
        || trailer_label(&signoff) != Some(SIGNOFF_LABEL)
    {
        return Err(ValidationError(
            "canonical trailer block must contain the two required lines".to_owned(),
        ));
    }
    let expected_signoff = format!("{SIGNOFF_LABEL}: {} <{}>", identity.name, identity.email);
    if signoff != expected_signoff {
        return Err(ValidationError(
            "canonical trailers disagree with the documented identity".to_owned(),
        ));
    }
    Ok(TrailerPolicy {
        identity,
        trailers: [coauthor, signoff],
    })
}

fn canonical_identity(text: &str) -> CheckResult<Identity> {
    let mut matches = text.match_indices(IDENTITY_MARKER);
    let Some((start, _)) = matches.next() else {
        return Err(ValidationError(
            "canonical author identity is missing or ambiguous".to_owned(),
        ));
    };
    if matches.next().is_some() {
        return Err(ValidationError(
            "canonical author identity is missing or ambiguous".to_owned(),
        ));
    }
    let value_start = start + IDENTITY_MARKER.len();
    let value = text
        .get(value_start..)
        .and_then(|tail| tail.split_once('`').map(|(identity, _)| identity))
        .ok_or_else(|| {
            ValidationError("canonical author identity is missing or ambiguous".to_owned())
        })?;
    let (name, email_with_close) = value.split_once(" <").ok_or_else(|| {
        ValidationError("canonical author identity is missing or ambiguous".to_owned())
    })?;
    let email = email_with_close.strip_suffix('>').ok_or_else(|| {
        ValidationError("canonical author identity is missing or ambiguous".to_owned())
    })?;
    if name.is_empty() || email.is_empty() || email.contains('<') || email.contains('>') {
        return Err(ValidationError(
            "canonical author identity is missing or ambiguous".to_owned(),
        ));
    }
    Ok(Identity {
        name: name.to_owned(),
        email: email.to_owned(),
    })
}

fn text_fenced_blocks(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split_terminator('\n').collect();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines.get(index).copied() == Some("```text") {
            let content_start = index + 1;
            let closing = lines
                .iter()
                .enumerate()
                .skip(content_start)
                .find_map(|(line_index, line)| (*line == "```").then_some(line_index));
            if let Some(content_end) = closing {
                let content = lines
                    .iter()
                    .take(content_end)
                    .skip(content_start)
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n");
                let content = if content_start < content_end {
                    format!("{content}\n")
                } else {
                    content
                };
                blocks.push(content);
                index = content_end + 1;
                continue;
            }
        }
        index += 1;
    }
    blocks
}

fn trailer_label(line: &str) -> Option<&str> {
    line.split_once(':').map(|(label, _)| label)
}

fn validate_message(message: &str, expected: &[String; 2]) -> CheckResult<()> {
    let lines = python_lines(message);
    let trailer_lines: Vec<String> = lines
        .iter()
        .filter(|line| has_trailer_label(line))
        .map(|line| (*line).to_owned())
        .collect();
    if trailer_lines.as_slice() != expected.as_slice() {
        return Err(ValidationError(
            "message must contain each exact canonical trailer once".to_owned(),
        ));
    }
    if lines.len() < 4 {
        return Err(ValidationError(
            "canonical trailers must form the final, separated block".to_owned(),
        ));
    }
    let mut trailing_lines = lines.iter().rev();
    let signoff = trailing_lines.next().copied();
    let coauthor = trailing_lines.next().copied();
    let separator = trailing_lines.next().copied();
    if signoff != expected.get(1).map(String::as_str)
        || coauthor != expected.first().map(String::as_str)
        || separator != Some("")
    {
        return Err(ValidationError(
            "canonical trailers must form the final, separated block".to_owned(),
        ));
    }
    if lines
        .first()
        .is_none_or(|subject| subject.chars().all(is_python_whitespace))
    {
        return Err(ValidationError("commit subject is missing".to_owned()));
    }
    Ok(())
}

fn validate_local_identities(root: &Path, expected: &Identity) -> CheckResult<()> {
    validate_local_identities_with_env(root, expected, &[], false)
}

fn validate_local_identities_with_env(
    root: &Path,
    expected: &Identity,
    environment: &[(OsString, OsString)],
    isolated_fixture: bool,
) -> CheckResult<()> {
    for (kind, label) in [("AUTHOR", "author"), ("COMMITTER", "committer")] {
        if git_identity(root, kind, label, environment, isolated_fixture)? != *expected {
            return Err(ValidationError(format!(
                "Git {label} identity differs from canonical policy"
            )));
        }
    }
    Ok(())
}

fn git_identity(
    root: &Path,
    kind: &str,
    label: &str,
    environment: &[(OsString, OsString)],
    isolated_fixture: bool,
) -> CheckResult<Identity> {
    let mut command = Command::new("git");
    if isolated_fixture {
        command.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
    }
    command
        .arg("var")
        .arg(format!("GIT_{kind}_IDENT"))
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .envs(environment.iter().map(|(key, value)| (key, value)));
    let output = command
        .output()
        .map_err(|_| ValidationError(format!("cannot read Git {label} identity")))?;
    if !output.status.success() {
        return Err(ValidationError(format!("cannot read Git {label} identity")));
    }
    let identity = String::from_utf8(output.stdout)
        .map_err(|_| ValidationError(format!("Git {label} identity is malformed")))?;
    parse_git_identity(&identity)
        .ok_or_else(|| ValidationError(format!("Git {label} identity is malformed")))
}

fn parse_git_identity(value: &str) -> Option<Identity> {
    let (name, remainder) = value.trim().rsplit_once(" <")?;
    let (email, _timestamp) = remainder.split_once("> ")?;
    if email.is_empty() || email.contains('<') || email.contains('>') {
        return None;
    }
    Some(Identity {
        name: name.to_owned(),
        email: email.to_owned(),
    })
}

fn io_error(error: &std::io::Error) -> ValidationError {
    ValidationError(error.to_string())
}

#[cfg(test)]
#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

#[cfg(test)]
#[path = "../tests/support/validate_commit_trailers.rs"]
mod tests;
