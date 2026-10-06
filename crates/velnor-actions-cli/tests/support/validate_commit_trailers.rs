use super::*;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

const CANONICAL_POLICY: &str = "The repository-local identity `Alexey Zhokhov <alexey@zhokhov.com>` is used.\n\
## Commit identity and trailers\n\
```text\n\
Co-authored-by: Codex <codex@openai.com>\n\
Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n\
```\n";

fn policy() -> CheckResult<TrailerPolicy> {
    canonical_policy(CANONICAL_POLICY)
}

fn valid_message(policy: &TrailerPolicy) -> String {
    let [coauthor, signoff] = &policy.trailers;
    format!("subject\n\n{coauthor}\n{signoff}\n")
}

fn assert_invalid(message: &str, trailers: &[String; 2]) {
    assert!(
        validate_message(message, trailers).is_err(),
        "message should be rejected: {message:?}"
    );
}

#[test]
fn exact_terminal_block_passes() -> CheckResult<()> {
    let policy = policy()?;
    assert!(validate_message(&valid_message(&policy), &policy.trailers).is_ok());
    Ok(())
}

#[test]
fn crlf_terminal_block_passes() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!("subject\r\n\r\n{coauthor}\r\n{signoff}\r\n");
    assert!(validate_message(&message, &policy.trailers).is_ok());
    Ok(())
}

#[test]
fn cli_accepts_local_identity_check_flag() -> Result<(), clap::Error> {
    let args = Args::try_parse_from([
        "validate-commit-trailers",
        "message.txt",
        "--check-local-identities",
    ])?;
    assert!(args.check_local_identities);
    Ok(())
}

#[test]
fn missing_trailers_fail() -> CheckResult<()> {
    let policy = policy()?;
    assert_invalid("subject\n\nbody", &policy.trailers);
    Ok(())
}

#[test]
fn short_signoff_name_fails() -> CheckResult<()> {
    let policy = policy()?;
    let message = "subject\n\nCo-authored-by: Codex <codex@openai.com>\nSigned-off-by: Alexey <alexey@zhokhov.com>";
    assert_invalid(message, &policy.trailers);
    Ok(())
}

#[test]
fn wrong_signoff_email_fails() -> CheckResult<()> {
    let policy = policy()?;
    let message = valid_message(&policy).replace("alexey@zhokhov.com", "wrong@example.com");
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn wrong_codex_coauthor_fails() -> CheckResult<()> {
    let policy = policy()?;
    let message =
        valid_message(&policy).replace("Codex <codex@openai.com>", "Codex <other@example.com>");
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn noncontiguous_trailers_fail() -> CheckResult<()> {
    let policy = policy()?;
    let message = valid_message(&policy).replace("\nSigned-off-by:", "\nbody\nSigned-off-by:");
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn reversed_trailers_fail() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    assert_invalid(
        &format!("subject\n\n{signoff}\n{coauthor}"),
        &policy.trailers,
    );
    Ok(())
}

#[test]
fn duplicate_trailers_fail() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, _] = &policy.trailers;
    assert_invalid(
        &format!("{}\n{coauthor}", valid_message(&policy)),
        &policy.trailers,
    );
    Ok(())
}

#[test]
fn body_decoy_trailer_fails() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!("subject\n\n{signoff}\n\n{coauthor}\n{signoff}");
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn unicode_word_suffix_is_not_a_trailer_label() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!("subject\n\nCo-authored-byé: prose\n\n{coauthor}\n{signoff}\n");
    assert!(validate_message(&message, &policy.trailers).is_ok());
    Ok(())
}

#[test]
fn unicode_line_break_exposes_body_trailer_decoy() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!(
        "subject\n\nDetails.\u{2028}Signed-off-by: Someone Else <else@example.com>\n\n{coauthor}\n{signoff}\n"
    );
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn unicode_casefold_alias_exposes_body_trailer_decoy() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!(
        "subject\n\nDetails.\nſigned-off-by: Someone Else <else@example.com>\n\n{coauthor}\n{signoff}\n"
    );
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn combining_mark_exposes_body_trailer_decoy() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!(
        "subject\n\nDetails.\nSigned-off-by\u{0345}: Someone Else <else@example.com>\n\n{coauthor}\n{signoff}\n"
    );
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn python_strip_control_space_rejects_empty_subject() -> CheckResult<()> {
    let policy = policy()?;
    let [coauthor, signoff] = &policy.trailers;
    let message = format!("\u{001f}\n\n{coauthor}\n{signoff}\n");
    assert_invalid(&message, &policy.trailers);
    Ok(())
}

#[test]
fn trailing_blank_line_fails() -> CheckResult<()> {
    let policy = policy()?;
    assert_invalid(&format!("{}\n", valid_message(&policy)), &policy.trailers);
    Ok(())
}

#[test]
fn ambiguous_policy_heading_fails() {
    assert!(canonical_policy(&format!("{CANONICAL_POLICY}\n{POLICY_HEADING}\n")).is_err());
}

#[test]
fn ambiguous_trailer_block_fails() {
    let extra = "```text\nCo-authored-by: Codex <codex@openai.com>\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n```\n";
    assert!(canonical_policy(&format!("{CANONICAL_POLICY}\n{extra}")).is_err());
}

#[test]
fn canonical_policy_accepts_universal_newlines() {
    for separator in ["\r\n", "\r"] {
        let policy_text = CANONICAL_POLICY.replace('\n', separator);
        assert!(canonical_policy(&policy_text).is_ok());
    }
}

#[test]
fn blank_line_inside_canonical_block_fails() {
    let malformed = CANONICAL_POLICY.replace(
        "Co-authored-by: Codex <codex@openai.com>\nSigned-off-by:",
        "Co-authored-by: Codex <codex@openai.com>\n\nSigned-off-by:",
    );
    assert!(canonical_policy(&malformed).is_err());
}

#[test]
fn trailing_blank_line_in_policy_block_fails() {
    let malformed = CANONICAL_POLICY.replace(
        "Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n```",
        "Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n\n```",
    );
    assert!(canonical_policy(&malformed).is_err());
}

#[test]
fn local_author_and_committer_match_identity() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new("matching")?;
    fixture.configure_identity("Alexey Zhokhov", "alexey@zhokhov.com")?;
    let policy = policy()?;
    validate_local_identities_with_env(
        &fixture.root,
        &policy.identity,
        &fixture.environment,
        true,
    )?;
    Ok(())
}

#[test]
fn local_identity_mismatch_fails() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new("mismatch")?;
    fixture.configure_identity("Alexey", "alexey@zhokhov.com")?;
    let policy = policy()?;
    assert!(
        validate_local_identities_with_env(
            &fixture.root,
            &policy.identity,
            &fixture.environment,
            true,
        )
        .is_err()
    );
    Ok(())
}

struct GitFixture {
    root: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl GitFixture {
    fn new(label: &str) -> io::Result<Self> {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-trailer-{}-{label}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let home = root.join("home");
        let hooks = root.join("disabled-hooks");
        fs::create_dir(&home)?;
        fs::create_dir(&hooks)?;
        let global_config = root.join("global-config");
        let system_config = root.join("system-config");
        fs::write(&global_config, "")?;
        fs::write(&system_config, "")?;
        let environment = vec![
            (OsString::from("HOME"), home.into_os_string()),
            (
                OsString::from("GIT_CONFIG_GLOBAL"),
                global_config.into_os_string(),
            ),
            (
                OsString::from("GIT_CONFIG_SYSTEM"),
                system_config.into_os_string(),
            ),
            (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")),
            (OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0")),
        ];
        let fixture = Self { root, environment };
        fixture.run_git(&["init", "--quiet", "--initial-branch=trailer-fixture"])?;
        let hooks_path = hooks.display().to_string();
        fixture.run_git(&["config", "--local", "core.hooksPath", &hooks_path])?;
        Ok(fixture)
    }

    fn configure_identity(&self, name: &str, email: &str) -> io::Result<()> {
        self.run_git(&["config", "--local", "user.name", name])?;
        self.run_git(&["config", "--local", "user.email", email])?;
        Ok(())
    }

    fn run_git(&self, arguments: &[&str]) -> io::Result<()> {
        let output = git_fixture::command(&self.root)?.args(arguments).output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other("Git command failed for trailer fixture"))
        }
    }
}

impl Drop for GitFixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("could not remove Git trailer fixture: {error}");
        }
    }
}
