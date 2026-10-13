use super::*;

#[test]
fn qualify_runs_artifact_without_rebuild_markers() {
    let argv = QualifyRequest::staged()
        .argv()
        .map_err(|err| err.to_string());
    assert!(argv.as_ref().is_ok_and(|argv| argv[0] == "sh"));
    assert!(argv.is_ok_and(|argv| {
        argv[2].contains("plan")
            && argv[2].contains("generate")
            && QualifyRequest::check_no_rebuild(&argv).is_ok()
    }));
}

#[test]
fn qualify_rebuild_attempt_rejected() {
    for bad in [
        "cargo test",
        "mbx build",
        "rustc x",
        "mise exec",
        "rebuild all",
    ] {
        let err = QualifyRequest::check_no_rebuild(&["sh".to_owned(), bad.to_owned()]);
        assert!(
            err.is_err_and(|err| err.to_string().contains("must_not_rebuild")),
            "{bad}"
        );
    }
}
