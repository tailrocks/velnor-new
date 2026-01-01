//! Private repository-maintenance gate input and flag validation.

use std::error::Error;
use std::path::Path;

use crate::impl_cli_gate::{assert_identical, stage_request};
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn repo_policy_operation_requires_allowlisted_action_and_root() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-repo-policy")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    for env in [
        vec![("VELNOR_INTERNAL_OP", "repo-policy-v1")],
        vec![
            ("VELNOR_INTERNAL_OP", "repo-policy-v1"),
            ("VELNOR_REPO_POLICY_ACTION", "toolchain-specs"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "repo-policy-v1"),
            ("VELNOR_REPO_POLICY_ACTION", "unknown"),
            ("VELNOR_REPO_POLICY_ROOT", "/"),
        ],
    ] {
        let gated = spawn_isolated(&[], &env, &tmp)?;
        assert_identical(&bare, &gated);
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize()?;
    let root = root.to_string_lossy().into_owned();
    let valid = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "repo-policy-v1"),
            ("VELNOR_REPO_POLICY_ACTION", "toolchain-specs"),
            ("VELNOR_REPO_POLICY_ROOT", &root),
        ],
        &tmp,
    )?;
    assert_eq!(
        code(&valid),
        0,
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert!(valid.stderr.is_empty());
    let specs = String::from_utf8(valid.stdout)?;
    assert!(specs.starts_with("rust@"), "{specs}");
    assert!(specs.contains(" mr-boxington@"), "{specs}");
    assert!(
        specs.contains(" aqua:nextest-rs/nextest/cargo-nextest@"),
        "{specs}"
    );

    let help = spawn_isolated(
        &["--help"],
        &[
            ("VELNOR_INTERNAL_OP", "repo-policy-v1"),
            ("VELNOR_REPO_POLICY_ACTION", "toolchain-specs"),
            ("VELNOR_REPO_POLICY_ROOT", &root),
        ],
        &tmp,
    )?;
    let plain_help = spawn_isolated(&["--help"], &[], &tmp)?;
    assert_identical(&plain_help, &help);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn repo_policy_boolean_flags_reject_malformed_values() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-repo-policy-flags")?;
    let message = stage_request(&tmp, "message.txt", "body\n")?;
    let root = "/";
    for (action, name) in [
        ("freshness", "VELNOR_REPO_POLICY_CHECK_UPSTREAM"),
        ("freshness", "VELNOR_REPO_POLICY_WITH_ADVISORIES"),
        (
            "trailer-policy",
            "VELNOR_REPO_POLICY_CHECK_LOCAL_IDENTITIES",
        ),
    ] {
        let mut env = vec![
            ("VELNOR_INTERNAL_OP", "repo-policy-v1"),
            ("VELNOR_REPO_POLICY_ACTION", action),
            ("VELNOR_REPO_POLICY_ROOT", root),
            (name, "true"),
        ];
        if action == "trailer-policy" {
            env.push((
                "VELNOR_REPO_POLICY_MESSAGE_PATH",
                message.to_str().unwrap_or("/"),
            ));
        }
        let output = spawn_isolated(&[], &env, &tmp)?;
        assert_eq!(code(&output), 1);
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(name), "{stderr}");
        assert!(stderr.contains("must be absent, 0, or 1"), "{stderr}");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn repo_policy_trailer_action_reaches_the_shared_owner() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-repo-policy-trailer")?;
    let message = stage_request(
        &tmp,
        "message.txt",
        "change: check the repository trailer contract\n\nCo-authored-by: Codex <codex@openai.com>\nSigned-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n",
    )?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let root = root.to_string_lossy().into_owned();
    let message = message.canonicalize()?;
    let message = message.to_string_lossy().into_owned();
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "repo-policy-v1"),
            ("VELNOR_REPO_POLICY_ACTION", "trailer-policy"),
            ("VELNOR_REPO_POLICY_ROOT", &root),
            ("VELNOR_REPO_POLICY_MESSAGE_PATH", &message),
        ],
        &tmp,
    )?;
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)?.contains("matches the canonical trailer policy"));
    cleanup(&tmp);
    Ok(())
}
