fn assert_classification(
    fixture: &Fixture,
    mut env: BTreeMap<String, String>,
    observer: &Path,
    key: &str,
    expected_miss: bool,
) -> Result<(), Box<dyn Error>> {
    env.extend([
        ("SHOULD_OBSERVE".to_owned(), "true".to_owned()),
        ("DERIVED_KEY".to_owned(), key.to_owned()),
        ("VALIDATED_CACHE_KEY".to_owned(), key.to_owned()),
        ("RESTORE_HIT".to_owned(), String::new()),
        ("MATCHED_KEY".to_owned(), String::new()),
        ("RESTORE_PRIMARY_KEY".to_owned(), key.to_owned()),
        ("RESTORE_CONCLUSION".to_owned(), "success".to_owned()),
        ("CONTROLLER_READY".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_READY_REASON".to_owned(),
            "exact_identity_and_cancel_window_ready".to_owned(),
        ),
        ("CONTROLLER_CANCEL_STATUS".to_owned(), "202".to_owned()),
        ("CONTROLLER_POST_REVALIDATED".to_owned(), "true".to_owned()),
        ("CONTROLLER_TERMINAL".to_owned(), "true".to_owned()),
        (
            "CONTROLLER_TERMINAL_STATE".to_owned(),
            "completed/cancelled".to_owned(),
        ),
        ("GENERATION".to_owned(), "velnor-mbx-1.22.0".to_owned()),
    ]);
    let output = fixture.output("classify");
    fs::write(&output, "")?;
    env.insert("GITHUB_OUTPUT".to_owned(), output.display().to_string());
    let classified = run_bash(
        scripts::OBSERVER_CLASSIFY,
        &fixture.root,
        &fixture.bin,
        &env.into_iter().collect::<Vec<_>>(),
    )?;
    assert!(classified.status.success());
    let result = fs::read_to_string(observer.join("result.json"))?;
    if expected_miss {
        assert!(result.contains("\"outcome\":\"MISS\""), "{result}");
        assert!(
            result.contains("\"reservation_state\":\"UNKNOWN\""),
            "{result}"
        );
    } else {
        assert!(result.contains("\"outcome\":\"NOT_RUN\""), "{result}");
    }
    Ok(())
}
