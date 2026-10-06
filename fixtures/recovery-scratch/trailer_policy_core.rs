//! Scratchable core for the PR #72 Rust trailer-policy port.
//!
//! This file is compiled directly with `rustc --test` until the shared
//! repository-policy foundation is frozen, then moved into that crate.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    name: String,
    email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalPolicy {
    identity: Identity,
    trailers: [String; 2],
}

const POLICY_HEADING: &str = "## Commit identity and trailers";
const IDENTITY_PREFIX: &str = "repository-local identity `";
const COAUTHOR: &str = "Co-authored-by";
const SIGNOFF: &str = "Signed-off-by";

pub(crate) fn parse_policy(text: &str) -> Result<CanonicalPolicy, String> {
    let identities = find_identities(text);
    if identities.len() != 1 {
        return Err("canonical author identity is missing or ambiguous".to_owned());
    }
    let identity = identities[0].clone();
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

pub(crate) fn validate_message(message: &str, policy: &CanonicalPolicy) -> Result<(), String> {
    let lines = git_records(message);
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
        || lines[len - 3] != ""
    {
        return Err("canonical trailers must form the final, separated block".to_owned());
    }
    if lines[0].trim().is_empty() {
        return Err("commit subject is missing".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_files_with_identity_lookup(
    root: &Path,
    message_path: &Path,
    check_local_identities: bool,
    mut identity_lookup: impl FnMut(&Path, &str) -> Result<String, String>,
) -> Result<(), String> {
    if !root.is_absolute() || !message_path.is_absolute() {
        return Err("repository and message paths must be absolute".to_owned());
    }
    let policy_path = root.join("docs/implemented/codex-agent-configuration.md");
    let policy_text = fs::read_to_string(policy_path)
        .map_err(|error| format!("cannot read canonical trailer policy ({error})"))?;
    let policy = parse_policy(&policy_text)?;
    let message = fs::read_to_string(message_path)
        .map_err(|error| format!("cannot read commit message ({error})"))?;
    validate_message(&message, &policy)?;
    if check_local_identities {
        let author = identity_lookup(root, "GIT_AUTHOR_IDENT")?;
        let committer = identity_lookup(root, "GIT_COMMITTER_IDENT")?;
        validate_local_identities(&policy.identity, &author, &committer)?;
    }
    Ok(())
}

pub(crate) fn validate_local_identities(
    expected: &Identity,
    author_record: &str,
    committer_record: &str,
) -> Result<(), String> {
    for record in [author_record, committer_record] {
        if parse_git_identity(record)? != *expected {
            return Err(
                "Git author or committer identity differs from canonical policy".to_owned(),
            );
        }
    }
    Ok(())
}

pub(crate) fn parse_git_identity(record: &str) -> Result<Identity, String> {
    let record = record.trim();
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
            break;
        };
        let candidate = &after_prefix[..end];
        if let Some((name, email_and_close)) = candidate.rsplit_once(" <") {
            if let Some(email) = email_and_close.strip_suffix('>') {
                if !name.is_empty() && !email.is_empty() {
                    identities.push(Identity {
                        name: name.to_owned(),
                        email: email.to_owned(),
                    });
                }
            }
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
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn git_records(message: &str) -> Vec<&str> {
    physical_records(message)
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
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
#[path = "trailer_policy_tests.rs"]
mod tests;
