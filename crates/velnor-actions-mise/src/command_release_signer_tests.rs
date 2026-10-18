use super::*;

#[test]
fn only_the_signing_command_receives_the_oidc_pair() -> Result<(), MiseError> {
    let parent = vec![
        (
            OsString::from("ACTIONS_ID_TOKEN_REQUEST_URL"),
            OsString::from("ambient-url"),
        ),
        (
            OsString::from("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
            OsString::from("ambient-token"),
        ),
        (
            OsString::from("GH_TOKEN"),
            OsString::from("ambient-gh-token"),
        ),
        (
            OsString::from("GITHUB_TOKEN"),
            OsString::from("ambient-github-token"),
        ),
    ];
    let verify = IsolatedCommand::repo_task("cosign", vec![OsString::from("verify-blob")], &[])?
        .with_policy(EnvPolicy::ReleaseSigner);
    let verify_env = EnvPolicy::ReleaseSigner.child_env(&parent, &verify.full_env());
    assert!(!has_env(&verify_env, "ACTIONS_ID_TOKEN_REQUEST_URL"));
    assert!(!has_env(&verify_env, "ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(!has_env(&verify_env, "GH_TOKEN"));
    assert!(!has_env(&verify_env, "GITHUB_TOKEN"));

    let sign = IsolatedCommand::repo_task("cosign", vec![OsString::from("sign-blob")], &[])?
        .with_policy(EnvPolicy::ReleaseSigner)
        .with_github_actions_oidc_values(
            OsString::from("explicit-url"),
            OsString::from("explicit-token"),
        )?;
    let sign_env = EnvPolicy::ReleaseSigner.child_env(&parent, &sign.full_env());
    assert_eq!(
        value(&sign_env, "ACTIONS_ID_TOKEN_REQUEST_URL"),
        Some("explicit-url")
    );
    assert_eq!(
        value(&sign_env, "ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        Some("explicit-token")
    );
    assert!(!has_env(&sign_env, "GH_TOKEN"));
    assert!(!has_env(&sign_env, "GITHUB_TOKEN"));
    Ok(())
}

fn has_env(env: &[(OsString, OsString)], key: &str) -> bool {
    env.iter().any(|(name, _)| name == key)
}

fn value<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a str> {
    env.iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.to_str())
}
