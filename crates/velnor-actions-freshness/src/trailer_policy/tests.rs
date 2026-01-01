use std::cell::Cell;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    parse_git_identity, parse_policy, validate_files_with_identity_lookup, validate_message,
};

const POLICY: &str = "# Agents\n\nThe repository-local identity `Alexey Zhokhov <alexey@zhokhov.com>`.\n\n## Commit identity and trailers\n\n```text\nCo-authored-by: Codex <codex@openai.com>\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n```\n";
const TRAILERS: &str =
    "Co-authored-by: Codex <codex@openai.com>\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>";

fn valid_policy() -> Option<super::CanonicalPolicy> {
    let result = parse_policy(POLICY);
    assert!(result.is_ok(), "{result:?}");
    result.ok()
}

fn valid_files() -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-trailer-policy-{}-{suffix}",
        std::process::id()
    ));
    let policy_path = root.join("docs/implemented/codex-agent-configuration.md");
    let message_path = root.join("message.txt");
    let policy_parent = policy_path
        .parent()
        .ok_or("policy path has no parent directory")?;
    fs::create_dir_all(policy_parent)?;
    fs::write(&policy_path, POLICY)?;
    fs::write(
        &message_path,
        format!("change: preserve provenance\n\n{TRAILERS}\n"),
    )?;
    Ok((root, message_path))
}

fn remove_fixture(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn exact_terminal_pair_accepts_lf_crlf_and_optional_final_newline() {
    let Some(policy) = valid_policy() else {
        return;
    };
    assert!(validate_message(&format!("change: keep provenance\n\n{TRAILERS}\n"), &policy).is_ok());
    assert!(validate_message(&format!("change: keep provenance\n\n{TRAILERS}"), &policy).is_ok());
    let crlf = TRAILERS.replace('\n', "\r\n");
    assert!(
        validate_message(
            &format!("change: keep provenance\r\n\r\n{crlf}\r\n"),
            &policy
        )
        .is_ok()
    );
}

#[test]
fn unicode_and_control_separators_remain_inside_one_git_record() {
    let Some(policy) = valid_policy() else {
        return;
    };
    for separator in [
        '\u{2028}', '\u{2029}', '\u{000b}', '\u{000c}', '\u{0085}', '\u{001c}', '\u{001d}',
        '\u{001e}', '\r',
    ] {
        let joined = TRAILERS.replace('\n', &separator.to_string());
        let message = format!("change: joined trailers\n\n{joined}\n");
        assert!(
            validate_message(&message, &policy).is_err(),
            "accepted separator U+{:04X}",
            u32::from(separator)
        );
    }
    let bare_cr = format!("change: trailing bare CR\n\n{TRAILERS}\r");
    assert!(validate_message(&bare_cr, &policy).is_err());
}

#[test]
fn missing_wrong_order_duplicate_body_decoy_and_nonterminal_pairs_fail() {
    let Some(policy) = valid_policy() else {
        return;
    };
    for message in [
        "change: missing\n\nbody\n",
        "change: reversed\n\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\nCo-authored-by: Codex <codex@openai.com>\n",
        "change: wrong author\n\nCo-authored-by: Codex <wrong@example.com>\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n",
        "change: short signoff\n\nCo-authored-by: Codex <codex@openai.com>\nSigned-off-by: Alexey <alexey@zhokhov.com>\n",
        "change: duplicate\n\n{TRAILERS}\n{TRAILERS}\n",
        "change: body decoy\n\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n\n{TRAILERS}\n",
        "change: nonterminal\n\n{TRAILERS}\nbody\n",
        "change: trailing blank\n\n{TRAILERS}\n\n",
        "\n\n{TRAILERS}\n",
    ] {
        let message = message.replace("{TRAILERS}", TRAILERS);
        assert!(
            validate_message(&message, &policy).is_err(),
            "accepted: {message:?}"
        );
    }
    let noncontiguous = "change: split trailer block\n\nCo-authored-by: Codex <codex@openai.com>\nReviewed-by: Someone\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n";
    assert!(validate_message(noncontiguous, &policy).is_err());
}

#[test]
fn policy_rejects_ambiguous_identity_heading_block_and_values() {
    assert!(parse_policy(&format!("{POLICY}\n{POLICY}")).is_err());
    assert!(parse_policy(&format!("{POLICY}\n## Commit identity and trailers\n")).is_err());
    assert!(parse_policy(&POLICY.replace("Co-authored-by:", "Co-author:")).is_err());
    assert!(parse_policy(&POLICY.replace("\nSigned-off-by:", "\n\nSigned-off-by:")).is_err());
    assert!(parse_policy(&POLICY.replace("```text", "```text\nextra\n```\n```text")).is_err());
    let mismatched = POLICY.replace(
        "Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>",
        "Signed-off-by: Alexey <alexey@example.com>",
    );
    assert!(parse_policy(&mismatched).is_err());
}

#[test]
fn author_and_committer_must_match_full_documented_identity() {
    let Some(policy) = valid_policy() else {
        return;
    };
    let expected = &policy.identity;
    let correct = "Alexey Zhokhov <alexey@zhokhov.com> 1 +0000\n";
    let abbreviated = "Alexey <alexey@zhokhov.com> 1 +0000\n";
    assert!(super::check_identity(expected, "author", correct).is_ok());
    assert!(super::check_identity(expected, "committer", correct).is_ok());
    assert!(super::check_identity(expected, "author", abbreviated).is_err());
    assert!(super::check_identity(expected, "committer", abbreviated).is_err());
    assert!(parse_git_identity("malformed identity").is_err());
}

#[test]
fn file_entrypoint_rejects_relative_paths_before_identity_lookup() {
    let called = Cell::new(false);
    let result = validate_files_with_identity_lookup(
        Path::new("."),
        Path::new("message.txt"),
        true,
        |_, _| {
            called.set(true);
            Ok(String::new())
        },
    );
    assert!(result.is_err());
    assert!(!called.get());
}

#[test]
fn valid_file_entrypoint_skips_identity_lookup_without_opt_in() -> Result<(), Box<dyn Error>> {
    let (root, message_path) = valid_files()?;
    let mut calls = Vec::new();
    let result = validate_files_with_identity_lookup(&root, &message_path, false, |_, variable| {
        calls.push(variable.to_owned());
        Ok(String::new())
    });
    assert!(result.is_ok(), "{result:?}");
    assert!(calls.is_empty());
    remove_fixture(&root)?;
    Ok(())
}

#[test]
fn valid_file_entrypoint_checks_author_then_committer_when_requested() -> Result<(), Box<dyn Error>>
{
    let (root, message_path) = valid_files()?;
    let mut calls = Vec::new();
    let ident = "Alexey Zhokhov <alexey@zhokhov.com> 1 +0000\n";
    let result = validate_files_with_identity_lookup(&root, &message_path, true, |_, variable| {
        calls.push(variable.to_owned());
        Ok(ident.to_owned())
    });
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(calls, ["GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"]);
    remove_fixture(&root)?;
    Ok(())
}
