//! Selector ownership and closed diff operand regression cases.

use super::{GitRequest, MiseError, OsString, diff_request, fixture::repo};

#[test]
fn diff_command_scrubs_git_selectors_and_keeps_owned_lock_setting() -> Result<(), String> {
    let fixture = repo("environment", "sha1", 2)?;
    let command = diff_request().command_in(&fixture.root);
    let selectors = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_NAMESPACE",
    ];
    let hostile: Vec<_> = selectors
        .iter()
        .map(|key| (OsString::from(*key), OsString::from("/hostile")))
        .collect();
    let environment = command.spawn_env(&hostile);
    assert!(
        !command
            .full_env()
            .iter()
            .any(|(name, _)| name == "GIT_INDEX_FILE")
    );
    for key in selectors {
        assert!(
            !environment
                .iter()
                .any(|(name, value)| name == key && value == "/hostile"),
            "{key} leaked"
        );
        let error = command
            .clone()
            .with_env(&[(OsString::from(key), OsString::from("/hostile"))])
            .expect_err("Git selector override must reject");
        assert!(matches!(error, MiseError::InvalidStepInput { field, .. } if field == key));
    }
    assert!(!environment.iter().any(|(name, _)| name == "GIT_INDEX_FILE"));
    assert!(
        environment
            .iter()
            .rev()
            .any(|(name, value)| name == "GIT_OPTIONAL_LOCKS" && value == "0")
    );
    let error = command
        .with_env(&[(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1"))])
        .expect_err("optional lock override must reject");
    assert!(
        matches!(error, MiseError::InvalidStepInput { field, .. } if field == "GIT_OPTIONAL_LOCKS")
    );
    Ok(())
}

#[test]
fn diff_rejects_unowned_flags_before_spawn() -> Result<(), String> {
    let fixture = repo("argument-rejection", "sha1", 2)?;
    for args in [
        vec!["-z"],
        vec!["--name-only"],
        vec!["--no-renames"],
        vec!["--cached"],
        vec!["--diff-filter=A"],
        vec!["--diff-filter=D"],
        vec!["--exit-code"],
    ] {
        let request = GitRequest::diff(args.into_iter().map(OsString::from).collect());
        let output = request
            .run_in(&fixture.root)
            .map_err(|error| error.to_string())?;
        assert!(output.success, "owned selector failed: {output:?}");
    }
    for args in [
        vec!["--output", "out.diff"],
        vec!["--ext-diff"],
        vec!["--no-ext-diff"],
        vec!["--textconv"],
        vec!["--no-textconv"],
        vec!["--unknown-flag"],
    ] {
        let request = GitRequest::diff(args.into_iter().map(OsString::from).collect());
        let error = request
            .run_in(&fixture.root)
            .expect_err("unowned diff flag must reject");
        assert!(
            matches!(error, MiseError::InvalidStepInput { .. }),
            "{error}"
        );
    }
    Ok(())
}
