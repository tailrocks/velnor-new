//! Canonical commit-trailer validation for repository-maintenance operations.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::process::run_bounded;

const POLICY_PATH: &str = "docs/implemented/codex-agent-configuration.md";
const POLICY_HEADING: &str = "## Commit identity and trailers";
const IDENTITY_PREFIX: &str = "repository-local identity `";
const COAUTHOR: &str = "Co-authored-by";
const SIGNOFF: &str = "Signed-off-by";
const IDENTITY_OUTPUT_CAP: usize = 4 * 1024;
const IDENTITY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    name: String,
    email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalPolicy {
    identity: Identity,
    trailers: [String; 2],
}

/// Validate one commit message and optionally compare the local Git identities.
pub(crate) fn run(root: &Path, message_path: &Path, check_local_identities: bool) -> i32 {
    let result = validate_files_with_identity_lookup(
        root,
        message_path,
        check_local_identities,
        git_identity,
    );
    match result {
        Ok(()) => {
            println!("commit message matches the canonical trailer policy");
            0
        }
        Err(error) => {
            eprintln!("commit trailer validation failed: {error}");
            1
        }
    }
}

fn validate_files_with_identity_lookup(
    root: &Path,
    message_path: &Path,
    check_local_identities: bool,
    mut identity_lookup: impl FnMut(&Path, &str) -> Result<String, String>,
) -> Result<(), String> {
    if !root.is_absolute() || !message_path.is_absolute() {
        return Err("repository and message paths must be absolute".to_owned());
    }
    let policy_path = root.join(POLICY_PATH);
    let policy_text = fs::read_to_string(policy_path)
        .map_err(|error| format!("cannot read canonical trailer policy ({error})"))?;
    let policy = parse_policy(&policy_text)?;
    let message = fs::read_to_string(message_path)
        .map_err(|error| format!("cannot read commit message ({error})"))?;
    validate_message(&message, &policy)?;
    if check_local_identities {
        let author = identity_lookup(root, "GIT_AUTHOR_IDENT")?;
        check_identity(&policy.identity, "author", &author)?;
        let committer = identity_lookup(root, "GIT_COMMITTER_IDENT")?;
        check_identity(&policy.identity, "committer", &committer)?;
    }
    Ok(())
}

fn parse_policy(text: &str) -> Result<CanonicalPolicy, String> {
    let mut identities = find_identities(text);
    if identities.len() != 1 {
        return Err("canonical author identity is missing or ambiguous".to_owned());
    }
    let identity = identities
        .pop()
        .ok_or_else(|| "canonical author identity is missing or ambiguous".to_owned())?;
    let heading_count = physical_records(text)
        .into_iter()
        .filter(|line| *line == POLICY_HEADING)
        .count();
    if heading_count != 1 {
        return Err("canonical trailer section is missing or ambiguous".to_owned());
    }
    let section = text
        .split_once(POLICY_HEADING)
        .map(|(_, section)| section)
        .ok_or_else(|| "canonical trailer section is missing".to_owned())?;
    let blocks = text_blocks(section);
    if blocks.len() != 1 {
        return Err("canonical trailer block is missing or ambiguous".to_owned());
    }
    let lines = physical_records(&blocks[0]);
    let labels = lines
        .iter()
        .map(|line| line.split_once(':').map_or(*line, |(label, _)| label))
        .collect::<Vec<_>>();
    if lines.len() != 2 || lines.iter().any(|line| line.is_empty()) {
        return Err("canonical trailer block must contain the two required lines".to_owned());
    }
    if labels != [COAUTHOR, SIGNOFF] {
        return Err("canonical trailer block must contain the two required lines".to_owned());
    }
    let expected_signoff = format!("{SIGNOFF}: {} <{}>", identity.name, identity.email);
    if lines[1] != expected_signoff {
        return Err("canonical trailers disagree with the documented identity".to_owned());
    }
    Ok(CanonicalPolicy {
        identity,
        trailers: [lines[0].to_owned(), lines[1].to_owned()],
    })
}

fn validate_message(message: &str, policy: &CanonicalPolicy) -> Result<(), String> {
    let lines = physical_records(message);
    let trailer_lines = lines
        .iter()
        .copied()
        .filter(|line| is_trailer_candidate(line))
        .collect::<Vec<_>>();
    let expected = policy
        .trailers
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if trailer_lines != expected {
        return Err("message must contain each exact canonical trailer once".to_owned());
    }
    let len = lines.len();
    if len < 4
        || lines[len - 2] != policy.trailers[0]
        || lines[len - 1] != policy.trailers[1]
        || !lines[len - 3].is_empty()
    {
        return Err("canonical trailers must form the final, separated block".to_owned());
    }
    if lines[0].trim().is_empty() {
        return Err("commit subject is missing".to_owned());
    }
    Ok(())
}

fn check_identity(expected: &Identity, kind: &str, record: &str) -> Result<(), String> {
    if parse_git_identity(record)? != *expected {
        return Err(format!("Git {kind} identity differs from canonical policy"));
    }
    Ok(())
}

fn git_identity(root: &Path, variable: &str) -> Result<String, String> {
    let kind = match variable {
        "GIT_AUTHOR_IDENT" => "author",
        "GIT_COMMITTER_IDENT" => "committer",
        _ => return Err("unknown Git identity variable".to_owned()),
    };
    let output = run_bounded(
        Command::new("git")
            .args(["var", variable])
            .current_dir(root),
        IDENTITY_OUTPUT_CAP,
        IDENTITY_TIMEOUT,
    )
    .map_err(|error| format!("cannot read Git {kind} identity ({error})"))?;
    if !output.status.success() {
        return Err(format!("cannot read Git {kind} identity"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("Git {kind} identity is not valid UTF-8"))
}

fn parse_git_identity(record: &str) -> Result<Identity, String> {
    let record = record.trim();
    if record.is_empty() || record.contains(['\n', '\r']) {
        return Err("Git identity is malformed".to_owned());
    }
    let separator = record
        .rfind(" <")
        .ok_or_else(|| "Git identity is malformed".to_owned())?;
    let name = &record[..separator];
    let tail = &record[separator + 2..];
    let (email, timestamp) = tail
        .split_once("> ")
        .ok_or_else(|| "Git identity is malformed".to_owned())?;
    if name.is_empty() || email.is_empty() || email.contains(['<', '>']) || timestamp.is_empty() {
        return Err("Git identity is malformed".to_owned());
    }
    Ok(Identity {
        name: name.to_owned(),
        email: email.to_owned(),
    })
}

fn find_identities(text: &str) -> Vec<Identity> {
    let mut identities = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(IDENTITY_PREFIX) {
        let after_prefix = &remaining[start + IDENTITY_PREFIX.len()..];
        let Some(end) = after_prefix.find('`') else {
            remaining = after_prefix;
            continue;
        };
        let candidate = &after_prefix[..end];
        if let Some((name, email_and_close)) = candidate.rsplit_once(" <")
            && let Some(email) = email_and_close.strip_suffix('>')
            && !name.is_empty()
            && !email.is_empty()
        {
            identities.push(Identity {
                name: name.to_owned(),
                email: email.to_owned(),
            });
        }
        remaining = &after_prefix[end + 1..];
    }
    identities
}

fn text_blocks(section: &str) -> Vec<String> {
    let lines = physical_records(section);
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while cursor + 1 < lines.len() {
        if lines[cursor] != "```text" {
            cursor += 1;
            continue;
        }
        let closing = (cursor + 1..lines.len()).find(|index| lines[*index] == "```");
        if let Some(end) = closing {
            blocks.push(lines[cursor + 1..end].join("\n"));
            cursor = end + 1;
        } else {
            cursor += 1;
        }
    }
    blocks
}

fn physical_records(text: &str) -> Vec<&str> {
    let mut records = Vec::new();
    let mut remaining = text;
    while let Some((record, tail)) = remaining.split_once('\n') {
        records.push(record.strip_suffix('\r').unwrap_or(record));
        remaining = tail;
    }
    if !remaining.is_empty() {
        records.push(remaining);
    }
    records
}

fn is_trailer_candidate(line: &str) -> bool {
    [COAUTHOR, SIGNOFF].into_iter().any(|label| {
        let Some(prefix) = line.get(..label.len()) else {
            return false;
        };
        if !prefix.eq_ignore_ascii_case(label) {
            return false;
        }
        line.get(label.len()..)
            .and_then(|tail| tail.chars().next())
            .is_none_or(|character| !character.is_alphanumeric() && character != '_')
    })
}

#[cfg(test)]
#[path = "trailer_policy/tests.rs"]
mod tests;
