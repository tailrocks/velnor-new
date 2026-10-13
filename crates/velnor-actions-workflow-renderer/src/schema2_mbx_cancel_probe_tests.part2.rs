#[test]
fn raw_api_steps_keep_multiline_source_and_scope_the_token() -> Result<(), Box<dyn Error>> {
    let env = BTreeMap::from([(
        "RUN_ID".to_owned(),
        "${{ steps.dispatch.outputs.workflow_run_id }}".to_owned(),
    )]);
    let step =
        render::token_bash_step("Dispatch", Some("dispatch"), scripts::DISPATCH, &env, None)?;
    let Yaml::Map(fields) = step else {
        return Err(io::Error::other("raw run step is not a mapping").into());
    };
    let run = fields
        .iter()
        .find(|(key, _)| key == "run")
        .map(|(_, value)| value)
        .ok_or_else(|| io::Error::other("raw run field missing"))?;
    let expected = format!(
        "{}\n{}\n{}",
        crate::schema2::mbx_stock_restore::STOCK_RESTORE_CLASSIFIER_SCRIPT,
        super::private_io::PRIVATE_IO_HELPERS,
        scripts::DISPATCH
    );
    assert_eq!(run, &Yaml::str(expected));
    let env = fields
        .iter()
        .find(|(key, _)| key == "env")
        .map(|(_, value)| value)
        .ok_or_else(|| io::Error::other("raw env field missing"))?;
    let Yaml::Map(env) = env else {
        return Err(io::Error::other("raw env is not a mapping").into());
    };
    assert!(
        env.iter().any(|(key, value)| {
            key == "GH_TOKEN" && value == &Yaml::str("${{ github.token }}")
        })
    );
    let override_token = BTreeMap::from([("GH_TOKEN".to_owned(), "bad".to_owned())]);
    assert!(render::token_bash_step("bad", None, "true", &override_token, None).is_err());
    Ok(())
}

#[test]
fn controller_artifact_download_targets_directory_and_reads_exact_file()
-> Result<(), Box<dyn Error>> {
    for (phase, artifact_name) in [
        (
            super::Phase::PreSave,
            "mbx-cancel-controller-receipt-pre-save",
        ),
        (
            super::Phase::DuringSave,
            "mbx-cancel-controller-receipt-during-save",
        ),
    ] {
        let step = super::probe_steps::download_controller_receipt_step(phase)?;
        let StepKind::Action { with, .. } = step.kind else {
            return Err(io::Error::other("receipt download is not an action").into());
        };
        assert_eq!(with.get("name").map(String::as_str), Some(artifact_name));
        assert_eq!(
            with.get("path").map(String::as_str),
            Some("${{ runner.temp }}/mbx-cancel-observer/controller-receipt")
        );
    }
    assert!(
        scripts::VALIDATE_CONTROLLER_RECEIPT
            .contains(r#"path="$RUNNER_TEMP/mbx-cancel-observer/controller-receipt/receipt.json""#)
    );
    Ok(())
}

#[test]
fn malformed_save_step_lists_never_request_step_logs() -> Result<(), Box<dyn Error>> {
    for mode in [
        "observer-duplicate-save",
        "observer-missing-save",
        "observer-object-steps",
    ] {
        let fixture = controller_fixtures::Fixture::new(mode, false)?;
        fixture.install_curl()?;
        let observer = fixture.root.join("mbx-cancel-observer/observer");
        let output = fixture.root.join("evidence.output");
        fs::write(&output, "")?;
        prepare_observer_root(&fixture.root, &fixture.bin, &fixture.env(&output, mode))?;
        fs::write(
            observer.join("cache-before.json"),
            "{\"count\":0,\"caches\":[]}\n",
        )?;
        let curl_log = fixture.root.join("curl.log");
        let summary = fixture.root.join("summary.md");
        fs::write(&summary, "")?;
        let mut env = fixture.env(&output, mode);
        env.extend([
            ("GH_TOKEN".to_owned(), "fixture-secret-token".to_owned()),
            (
                "VALIDATED_CACHE_KEY".to_owned(),
                controller_fixtures::expected_key(),
            ),
            ("CURL_LOG".to_owned(), curl_log.display().to_string()),
            ("CURL_LOCATION_MODE".to_owned(), "missing".to_owned()),
            ("CHILD_SOURCE_SHA".to_owned(), "c".repeat(40)),
            ("CHILD_ACTOR".to_owned(), "github-actions[bot]".to_owned()),
            ("CONTROLLER_CANCEL_REQUESTED".to_owned(), "true".to_owned()),
            (
                "CONTROLLER_CANCEL_AT".to_owned(),
                "2026-10-04T00:00:10Z".to_owned(),
            ),
            ("CONTROLLER_BEFORE_COUNT".to_owned(), "0".to_owned()),
            (
                "GITHUB_STEP_SUMMARY".to_owned(),
                summary.display().to_string(),
            ),
        ]);
        let script = scripts::observer_evidence();
        let result = run_bash(&script, &fixture.root, &fixture.bin, &env)?;
        assert!(
            result.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            fs::read_to_string(observer.join("child-evidence.json"))?
                .contains("\"progress_before_runner_cancel_error\":false")
        );
        let requests = if curl_log.exists() {
            fs::read_to_string(&curl_log)?
        } else {
            String::new()
        };
        assert!(
            requests.contains("child-run-api authorized=true"),
            "{mode}: {requests}"
        );
        assert!(
            requests.contains("child-jobs-api authorized=true"),
            "{mode}: {requests}"
        );
        assert!(
            !requests.contains("/steps/"),
            "{mode} fetched a step log: {requests}"
        );
        fs::remove_dir_all(fixture.root)?;
    }
    Ok(())
}
